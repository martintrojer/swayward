//! Sway container commands on the workspace's tiling tree and floating trees.

use super::*;

impl<W: LayoutElement> Workspace<W> {
    pub fn add_tile_at_drop(
        &mut self,
        tile: Tile<W>,
        target: NodeId,
        edge: ResizeEdge,
        activate: bool,
    ) {
        self.enter_output_for_window(tile.window());
        self.tiling.add_tile_at_drop(tile, target, edge, activate);
        if activate {
            self.floating_is_active = FloatingActive::No;
        }
    }

    pub fn add_tiling_tile(&mut self, tile: Tile<W>, activate: bool) {
        self.enter_output_for_window(tile.window());
        self.tiling
            .add_tile_with_activation(tile, InsertTarget::Focused, activate);
        if activate {
            self.floating_is_active = FloatingActive::No;
        }
    }

    pub fn detach_tiling_subtree(
        &mut self,
        id: NodeId,
    ) -> Option<(DetachedSubtree<W>, Option<NodeId>)> {
        let detached = self.tiling.detach_subtree(id)?;
        if let Some(output) = &self.output {
            detached
                .0
                .for_each_window(|window| window.output_leave(output));
        }
        self.update_focus_floating_tiling_after_removing(false);
        Some(detached)
    }

    pub fn attach_tiling_subtree(
        &mut self,
        subtree: DetachedSubtree<W>,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        self.attach_tiling_subtree_at(subtree, None)
    }

    pub fn swap_tiling_nodes(&mut self, first: NodeId, second: NodeId) -> Result<(), &'static str> {
        if self.tiling.contains(first) && self.tiling.contains(second) {
            return self.tiling.swap_nodes(first, second);
        }
        self.floating.swap_nodes(first, second)
    }

    pub fn detach_tiling_subtree_for_swap(
        &mut self,
        id: NodeId,
    ) -> Option<(DetachedSubtree<W>, crate::layout::tiling_tree::DetachedSlot)> {
        let detached = self.tiling.detach_subtree_for_swap(id)?;
        if let Some(output) = &self.output {
            detached
                .0
                .for_each_window(|window| window.output_leave(output));
        }
        Some(detached)
    }

    pub fn attach_tiling_subtree_for_swap(
        &mut self,
        subtree: DetachedSubtree<W>,
        slot: crate::layout::tiling_tree::DetachedSlot,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        if let Some(output) = &self.output {
            subtree.for_each_window(|window| window.output_enter(output));
        }
        self.tiling.attach_subtree_for_swap(subtree, slot)
    }

    pub fn attach_tiling_subtree_at(
        &mut self,
        subtree: DetachedSubtree<W>,
        target: Option<NodeId>,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        if let Some(output) = &self.output {
            subtree.for_each_window(|window| window.output_enter(output));
        }
        let fullscreen = subtree.has_fullscreen();
        if fullscreen {
            self.disable_fullscreen();
        }
        self.floating_is_active = FloatingActive::No;
        self.tiling.attach_subtree_at(subtree, target)
    }

    pub fn clear_floating_tree_fullscreen(&mut self, root: NodeId) {
        let Some(tree) = self.floating.tree_mut(root) else {
            return;
        };
        if let Some(fullscreen) = tree.fullscreen_node() {
            tree.set_node_fullscreen(fullscreen, None);
        }
    }

    pub fn remove_floating_tree(&mut self, root: NodeId) -> Option<RemovedFloatingTree<W>> {
        let removed = self.floating.remove_tree_for_transfer(root)?;
        if let Some(output) = &self.output {
            for (_, window) in removed.tree.windows() {
                window.output_leave(output);
            }
        }
        self.update_focus_floating_tiling_after_removing(true);
        Some(removed)
    }

    pub fn add_floating_tree(
        &mut self,
        removed: RemovedFloatingTree<W>,
        remap_position: bool,
    ) -> NodeId {
        if let Some(output) = &self.output {
            for (_, window) in removed.tree.windows() {
                window.output_enter(output);
            }
        }
        let (root, remapped) = self.floating.add_removed_tree(removed, remap_position);
        debug_assert!(remapped.is_empty());
        self.floating_is_active = FloatingActive::Yes;
        root
    }

    pub fn remove_active_tiling_tile(&mut self) -> Option<Tile<W>> {
        if self.floating_is_active.get() {
            return None;
        }
        let id = self.tiling.active_window()?.id().clone();
        let tile = self.tiling.remove_tile(&id, Transaction::new())?;
        if let Some(output) = &self.output {
            tile.window().output_leave(output);
        }
        self.update_focus_floating_tiling_after_removing(false);
        Some(tile)
    }

    pub fn focus_parent(&mut self) -> bool {
        if self.floating_is_active.get() {
            if self.floating.focus_parent() {
                return true;
            }
            // A floating root's parent is the workspace (`focus_parent`,
            // sway/commands/focus.c:339-351), even when nothing is tiled. An empty tiling tree
            // keeps no focus of its own, so the raised-but-inactive floating state alone stands
            // for the focused workspace there.
            self.floating_is_active = FloatingActive::NoButRaised;
            if !self.tiling.is_empty() {
                self.tiling.focus_root();
            }
            true
        } else {
            let changed = self.tiling.focus_parent();
            if self.tiling.root_is_focused() {
                self.floating_is_active = FloatingActive::No;
            }
            changed
        }
    }

    pub fn scroll_tab_indicator(&mut self, window: &W::Id, steps: i32) -> Option<W::Id> {
        (!self.floating_is_active.get())
            .then(|| self.tiling.scroll_tab_indicator(window, steps))
            .flatten()
    }

    pub fn focus_next_prev_sibling(&mut self, next: bool) -> bool {
        !self.floating_is_active.get() && self.tiling.focus_next_prev_sibling(next)
    }

    pub fn focus_child(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_child()
        } else if self.is_workspace_focused()
            && self.floating_is_active == FloatingActive::NoButRaised
        {
            self.floating_is_active = FloatingActive::Yes;
            true
        } else {
            self.tiling.focus_child()
        }
    }

    pub fn focus_tiling_node(&mut self, id: crate::layout::tiling_tree::NodeId) -> bool {
        if self
            .tiling_node_windows(id)
            .is_some_and(|windows| !windows.is_empty())
        {
            self.floating_is_active = FloatingActive::No;
            self.tiling.set_focus(id);
            true
        } else {
            false
        }
    }

    pub fn focused_tiling_node(&self) -> Option<crate::layout::tiling_tree::NodeId> {
        (!self.floating_is_active.get())
            .then(|| self.tiling.focus())
            .flatten()
    }

    pub fn focused_container_node(&self) -> Option<crate::layout::tiling_tree::NodeId> {
        if self.floating_is_active.get() {
            self.floating.focused_container_node()
        } else if self.floating_is_active == FloatingActive::NoButRaised {
            self.tiling.focus().filter(|id| self.tiling.is_root(*id))
        } else {
            self.focused_tiling_node()
        }
    }

    pub fn focused_floating_tree_child(&self) -> bool {
        self.floating_is_active.get() && self.floating.focused_tree_child()
    }

    pub fn is_workspace_focused(&self) -> bool {
        !self.floating_is_active.get()
            && (self.tiling.root_is_focused()
                || self.tiling.is_empty() && self.floating_is_active == FloatingActive::NoButRaised)
    }

    pub fn toggle_tiling_target_layout(
        &mut self,
        id: crate::layout::tiling_tree::NodeId,
        toggle: &swayward_ipc::command::LayoutToggle,
        container: bool,
    ) -> bool {
        if container {
            self.tiling.toggle_node_layout(id, toggle)
        } else {
            self.tiling.toggle_target_layout(id, toggle)
        }
    }

    pub fn restore_tiling_target_layout(
        &mut self,
        id: crate::layout::tiling_tree::NodeId,
        container: bool,
    ) -> bool {
        if container {
            self.tiling.restore_node_layout(id)
        } else {
            self.tiling.restore_target_layout(id)
        }
    }

    pub fn tiling_node_windows(
        &self,
        id: crate::layout::tiling_tree::NodeId,
    ) -> Option<Vec<W::Id>> {
        self.tiling.contains(id).then(|| {
            self.tiling
                .windows()
                .filter(|(leaf, _)| self.tiling.contains_node(id, *leaf))
                .map(|(_, window)| window.id().clone())
                .collect()
        })
    }

    pub fn focus_from_output_direction(
        &mut self,
        direction: crate::layout::tiling_tree::Direction,
    ) -> bool {
        if self.tiling.is_empty() {
            return false;
        }
        self.floating_is_active = FloatingActive::No;
        self.tiling.focus_from_output_direction(direction)
    }

    pub fn focus_next_or_prev(&mut self, next: bool) -> Option<bool> {
        if self.floating_is_active.get() {
            return None;
        }
        Some(self.tiling.focus_next_or_prev(next))
    }

    pub fn focus_left_without_wrap(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_left()
        } else {
            self.tiling.focus_direction_without_wrap(Direction::Left)
        }
    }

    pub fn focus_right_without_wrap(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_right()
        } else {
            self.tiling.focus_direction_without_wrap(Direction::Right)
        }
    }

    pub fn focus_first_root_child(&mut self) {
        if self.floating_is_active.get() {
            self.floating.focus_leftmost();
        } else {
            self.tiling.focus_first_root_child();
        }
    }

    pub fn focus_last_root_child(&mut self) {
        if self.floating_is_active.get() {
            self.floating.focus_rightmost();
        } else {
            self.tiling.focus_last_root_child();
        }
    }

    pub fn focus_right_or_first_root_child(&mut self) {
        if !self.focus_right() {
            self.focus_first_root_child();
        }
    }

    pub fn focus_left_or_last_root_child(&mut self) {
        if !self.focus_left() {
            self.focus_last_root_child();
        }
    }

    pub fn focus_root_child(&mut self, index: usize) {
        if self.floating_is_active.get() {
            self.focus_tiling();
        }
        self.tiling.focus_root_child(index);
    }

    pub fn focus_window_in_parent(&mut self, index: u8) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.focus_window_in_parent(index);
    }

    pub fn focus_down_without_wrap(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_down()
        } else {
            self.tiling.focus_direction_without_wrap(Direction::Down)
        }
    }

    pub fn focus_up_without_wrap(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_up()
        } else {
            self.tiling.focus_direction_without_wrap(Direction::Up)
        }
    }

    pub fn move_window_in_direction(
        &mut self,
        window: &W::Id,
        direction: Direction,
        pixels: f64,
    ) -> bool {
        if self.floating.has_window(window) {
            if self.floating.move_tree_window(window, direction).is_some() {
                return true;
            }
            let (x, y) = match direction {
                Direction::Left => (-pixels, 0.),
                Direction::Right => (pixels, 0.),
                Direction::Up => (0., -pixels),
                Direction::Down => (0., pixels),
            };
            self.floating.move_window(
                Some(window),
                PositionChange::AdjustFixed(x),
                PositionChange::AdjustFixed(y),
                true,
            );
            true
        } else {
            self.tiling.move_window_direction(window, direction)
        }
    }

    pub fn move_focused_root_child_to_first(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.move_focused_to_first();
    }

    pub fn move_focused_root_child_to_last(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.move_focused_to_last();
    }

    pub fn move_focused_root_child_to_index(&mut self, index: usize) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.move_focused_to_index(index.saturating_sub(1));
    }

    pub fn nest_or_unnest_window_left(&mut self, window: Option<&W::Id>) {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            return;
        }
        self.tiling.nest_or_unnest_window_left(window);
    }

    pub fn nest_or_unnest_window_right(&mut self, window: Option<&W::Id>) {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            return;
        }
        self.tiling.nest_or_unnest_window_right(window);
    }

    pub fn nest_focused_window(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.nest_focused_window();
    }

    pub fn unnest_focused_window(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.unnest_focused_window();
    }

    pub fn swap_window_horizontal(&mut self, right: bool) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.swap_window_horizontal(right);
    }

    pub fn toggle_focused_tabbed_display(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.toggle_focused_tabbed_display();
    }

    pub fn set_focused_layout(
        &mut self,
        layout: crate::layout::tiling_tree::Layout,
    ) -> Vec<(NodeId, NodeId)> {
        if self.floating_is_active.get() {
            self.floating
                .focused_child_tree_mut()
                .map(|tree| tree.set_focused_layout(layout))
                .unwrap_or_default()
        } else {
            self.tiling.set_focused_layout(layout)
        }
    }

    pub fn split_focused(&mut self, layout: crate::layout::tiling_tree::Layout) {
        if self.floating_is_active.get() {
            self.floating.split_active(layout);
        } else {
            self.tiling.split_focused(layout);
        }
    }

    pub fn flatten_focused_parent(&mut self) -> Option<(NodeId, NodeId)> {
        (!self.floating_is_active.get())
            .then(|| self.tiling.focus())
            .flatten()
            .and_then(|focus| self.tiling.flatten_parent(focus))
    }

    pub fn toggle_focused_layout(
        &mut self,
        toggle: &swayward_ipc::command::LayoutToggle,
    ) -> Vec<(NodeId, NodeId)> {
        if self.floating_is_active.get() {
            self.floating
                .focused_child_tree_mut()
                .map(|tree| tree.toggle_focused_layout(toggle))
                .unwrap_or_default()
        } else {
            self.tiling.toggle_focused_layout(toggle)
        }
    }

    pub fn restore_focused_split_layout(&mut self) -> Option<Vec<(NodeId, NodeId)>> {
        if self.floating_is_active.get() {
            self.floating
                .focused_child_tree_mut()?
                .restore_focused_split_layout()
        } else {
            self.tiling.restore_focused_split_layout()
        }
    }

    pub fn toggle_focused_layout_split(&mut self) -> Vec<(NodeId, NodeId)> {
        if self.floating_is_active.get() {
            self.floating
                .focused_child_tree_mut()
                .map(crate::layout::tiling_tree::TilingTree::toggle_focused_layout_split)
                .unwrap_or_default()
        } else {
            self.tiling.toggle_focused_layout_split()
        }
    }

    pub fn toggle_focused_split(&mut self) {
        if !self.floating_is_active.get() {
            self.tiling.toggle_focused_split();
        }
    }

    pub fn set_focused_display(&mut self, display: ColumnDisplay) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.set_focused_display(display);
    }

    pub fn set_focused_width(&mut self, change: SizeChange) {
        if self.floating_is_active.get() {
            self.floating
                .set_window_width(None, change, true, self.view_size.to_i32_round());
        } else {
            self.tiling.set_window_width(None, change);
        }
    }

    pub fn set_window_size_sway(
        &mut self,
        window: &W::Id,
        width: Option<SizeChange>,
        height: Option<SizeChange>,
        automatic_maximum: Size<i32, Logical>,
    ) {
        if self.is_floating(window) {
            if let Some(change) = width {
                self.floating
                    .set_window_outer_width(window, change, automatic_maximum);
            }
            if let Some(change) = height {
                self.floating
                    .set_window_outer_height(window, change, automatic_maximum);
            }
        } else {
            self.tiling.set_window_size_sway(window, width, height);
        }
    }

    pub fn resize_window_edge(
        &mut self,
        window: Option<&W::Id>,
        edge: ResizeEdge,
        change: SizeChange,
    ) -> Option<bool> {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            Some(self.floating.resize_window_edge(window, edge, change))
        } else {
            Some(self.tiling.resize_window_edge(window, edge, change))
        }
    }

    pub fn expand_focused_to_available_width(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.toggle_full_width();
    }

    pub(in crate::layout) fn focus_workspace_node(&mut self) {
        self.floating_is_active = FloatingActive::No;
    }

    /// Moves a child of a multi-window floating group into the tiling tree. Sway treats only
    /// the group root as floating (`container_is_floating`, sway/tree/container.c:1041-1049),
    /// so a command aimed at the child moves it as a tiled container and leaves its siblings in
    /// the group. Returns false when the window is not such a child.
    pub fn detach_floating_group_child(&mut self, window: &W::Id) -> bool {
        let Some(root) = self.floating.tree_root_for_window(window) else {
            return false;
        };
        if self
            .floating
            .tree_window_ids(root)
            .is_none_or(|windows| windows.len() < 2)
        {
            return false;
        }
        let removed = self.floating.remove_tile(window, Transaction::new());
        let mut tile = removed.tile;
        tile.restore_to_floating = false;
        self.tiling
            .add_tile_with_activation(tile, InsertTarget::Focused, true);
        self.floating_is_active = FloatingActive::No;
        true
    }

    pub fn floating_tree_root_for_window(&self, window: &W::Id) -> Option<NodeId> {
        self.floating.tree_root_for_window(window)
    }

    pub fn floating_transfer_window_ids(&self) -> Vec<W::Id> {
        self.floating.transfer_window_ids()
    }

    pub fn window_is_floating_root(&self, window: &W::Id) -> bool {
        self.floating.window_is_floating_root(window) || self.floating.window_is_tree_root(window)
    }

    pub fn focused_floating_tree_root(&self) -> Option<NodeId> {
        self.active_window()
            .and_then(|window| self.floating.tree_root_for_window(window.id()))
    }

    pub fn contains_swap_node(&self, id: crate::layout::tiling_tree::NodeId) -> bool {
        self.tiling.contains(id) || self.floating.tree_root_for_node(id).is_some()
    }

    pub fn swap_node_for_window(
        &self,
        window: &W::Id,
    ) -> Option<crate::layout::tiling_tree::NodeId> {
        self.tiling.node_for_window(window).or_else(|| {
            let root = self.floating.tree_root_for_window(window)?;
            self.floating.tree(root)?.node_for_window(window)
        })
    }

    pub fn is_tiling_split(&self, id: crate::layout::tiling_tree::NodeId) -> bool {
        self.tiling.is_split(id)
            || self
                .floating
                .tree_root_for_node(id)
                .and_then(|root| self.floating.tree(root))
                .is_some_and(|tree| tree.is_split(id))
    }

    pub fn set_container_floating(&mut self, node: NodeId, floating: bool) -> Option<NodeId> {
        if let Some(root) = self.floating.tree_root_for_node(node) {
            if floating {
                return Some(root);
            }
            let subtree = self.floating.remove_tree(root)?;
            if let Some(output) = &self.output {
                subtree.for_each_window(|window| window.output_enter(output));
            }
            if subtree.has_fullscreen() {
                self.disable_fullscreen();
            }
            self.floating_is_active = FloatingActive::No;
            let (root, _) = self.tiling.attach_unfloated_subtree(subtree);
            if self.floating.is_empty() {
                self.floating_is_active = FloatingActive::No;
            }
            return Some(root);
        }
        if !floating {
            return self.tiling.contains(node).then_some(node);
        }
        let (subtree, old_parent) = self.detach_tiling_subtree(node)?;
        self.tiling.finish_subtree_detach(old_parent);
        let size = Size::from((
            self.working_area.size.w * 0.5,
            self.working_area.size.h * 0.75,
        ));
        let rect = Rectangle::new(
            self.working_area.loc
                + (self.working_area.size.to_point() - size.to_point()).downscale(2.),
            size,
        );
        let (root, remapped) = self.floating.add_tree(subtree, rect);
        debug_assert!(remapped.is_empty());
        self.floating_is_active = FloatingActive::Yes;
        Some(root)
    }
}

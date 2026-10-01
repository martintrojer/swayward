use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn add_tile(&mut self, tile: Tile<W>, target: InsertTarget) -> NodeId {
        self.add_tile_with_activation(tile, target, true)
    }

    pub fn add_tile_at_drop(
        &mut self,
        tile: Tile<W>,
        target: NodeId,
        edge: ResizeEdge,
        activate: bool,
    ) -> NodeId {
        let layout = if edge.intersects(ResizeEdge::LEFT | ResizeEdge::RIGHT) {
            Layout::SplitH
        } else {
            Layout::SplitV
        };
        // tiled_drop_target computes the target before the drop completes; a
        // target that has since gone falls back to an ordinary insertion.
        let Some(parent) = self
            .nodes
            .get(&target)
            .map(|node| node.parent.unwrap_or(self.root))
        else {
            return self.add_tile_with_activation(tile, InsertTarget::Focused, activate);
        };
        if !matches!(
            self.nodes.get(&parent).map(|node| &node.value),
            Some(TreeNode::Split { layout: current, .. }) if *current == layout
        ) {
            self.split(target, layout);
        }
        let id = self.add_tile_with_activation(tile, InsertTarget::Node(target), activate);
        if edge.intersects(ResizeEdge::LEFT | ResizeEdge::TOP) {
            let parent = self
                .nodes
                .get(&id)
                .and_then(|node| node.parent)
                .expect("invariant: an inserted tile has a parent");
            let first = self
                .child_index(parent, target)
                .expect("invariant: the drop target remains a child of the insertion parent");
            let second = self
                .child_index(parent, id)
                .expect("invariant: the inserted tile is a child of its parent");
            let TreeNode::Split {
                children, percents, ..
            } = &mut self
                .nodes
                .get_mut(&parent)
                .expect("invariant: every child parent is present in the arena")
                .value
            else {
                unreachable!()
            };
            children.swap(first, second);
            percents.swap(first, second);
        }
        id
    }

    pub fn add_tile_right_of(
        &mut self,
        right_of: &W::Id,
        tile: Tile<W>,
        activate: bool,
    ) -> Option<NodeId> {
        let target = self.node_for_window(right_of)?;
        Some(self.add_tile_with_activation(tile, InsertTarget::Node(target), activate))
    }

    pub fn add_tile_to_subtree(
        &mut self,
        subtree: NodeId,
        tile: Tile<W>,
        activate: bool,
    ) -> Option<NodeId> {
        self.nodes
            .contains_key(&subtree)
            .then(|| self.add_tile_with_activation(tile, InsertTarget::Node(subtree), activate))
    }

    pub fn add_tile_with_activation(
        &mut self,
        mut tile: Tile<W>,
        target: InsertTarget,
        activate: bool,
    ) -> NodeId {
        self.interactive_resize = None;
        self.has_had_tile = true;
        tile.update_config(self.view_size, self.scale, self.options.clone());
        let pending_mode = tile.window().pending_sizing_mode();
        let fullscreen = self.fullscreen_node();
        let previous_focus = self.focus;
        let old_geometries = self.compute_geometry();
        if !self.fullscreen_layout_wrappers.is_empty() {
            self.pre_layout_ipc_rects.clear();
        }
        let id = self.alloc(Node {
            parent: None,
            value: TreeNode::Leaf {
                tile: Box::new(tile),
            },
        });
        let (parent, after) = self.insertion_slot(target);
        self.insert_child(parent, id, after);
        // Sway never focuses a view mapped while its workspace has a
        // fullscreen container (`should_focus`, `sway/tree/view.c:706-709`),
        // but only a view added directly to the workspace stays unarranged:
        // one added to a container is arranged with its siblings
        // (`arrange_container(parent)`, `sway/tree/view.c:931-940`).
        let focus_blocked = fullscreen.is_some();
        let mapped_under_fullscreen = focus_blocked && parent == self.root;
        if fullscreen
            .is_some_and(|fullscreen| self.divides_box_under_fullscreen(parent, fullscreen))
        {
            self.fullscreen_tile_slot = true;
        }
        if parent == self.root {
            if let Some(layout) = match self.options.layout.workspace_layout {
                swayward_config::WorkspaceLayout::Default => None,
                swayward_config::WorkspaceLayout::Stacking => Some(Layout::Stacked),
                swayward_config::WorkspaceLayout::Tabbed => Some(Layout::Tabbed),
            } {
                self.wrap_node(id, layout);
            }
        }
        self.place_new_leaf_in_focus_order(id, activate, focus_blocked, previous_focus);
        if mapped_under_fullscreen && !pending_mode.is_fullscreen() {
            self.mapped_under_fullscreen.insert(id);
        }
        if pending_mode.is_maximized() {
            self.pending_modes.insert(
                id,
                PendingMode {
                    maximized: true,
                    ..PendingMode::default()
                },
            );
        }
        self.compact_tree();
        if pending_mode.is_fullscreen() {
            self.replace_fullscreen_state(id, Some(FullscreenMode::Workspace));
        }
        self.animate_geometry_changes(old_geometries, Some(id));
        self.request_window_sizes();
        id
    }

    /// Whether a leaf added to `parent` takes a share of a split box inside `fullscreen`. Only
    /// split layouts divide the parent box; tabbed and stacked children keep the full box
    /// (`apply_tabbed_layout`, sway/tree/arrange.c:163-187).
    fn divides_box_under_fullscreen(&self, parent: NodeId, fullscreen: NodeId) -> bool {
        parent != self.root
            && parent != fullscreen
            && self.contains_node(parent, fullscreen)
            && matches!(
                self.nodes.get(&parent).map(|node| &node.value),
                Some(TreeNode::Split {
                    layout: Layout::SplitH | Layout::SplitV,
                    ..
                })
            )
    }

    /// Focuses a new leaf, or ranks it behind the kept focus: second when merely not
    /// activated, last when a fullscreen container blocks focus.
    fn place_new_leaf_in_focus_order(
        &mut self,
        id: NodeId,
        activate: bool,
        focus_blocked: bool,
        previous_focus: Option<NodeId>,
    ) {
        match previous_focus {
            Some(previous_focus) if !activate || focus_blocked => {
                self.focus_history.retain(|candidate| *candidate != id);
                if focus_blocked {
                    self.focus_history.push(id);
                } else {
                    self.focus_history
                        .insert(1.min(self.focus_history.len()), id);
                }
                self.focus = Some(previous_focus);
            }
            _ => self.set_focus_id(Some(id)),
        }
    }

    fn insertion_slot(&self, target: InsertTarget) -> (NodeId, Option<NodeId>) {
        let target = match target {
            InsertTarget::Focused => self.focus,
            InsertTarget::Node(id) => Some(id),
            InsertTarget::MoveDestination => {
                if let Some(focus) = self.focus.filter(|focus| self.is_split(*focus)) {
                    return (focus, None);
                }
                self.focus
            }
        };
        let parent = target
            .and_then(|id| self.nodes.get(&id)?.parent)
            .unwrap_or(self.root);
        let after = target
            .filter(|target| self.nodes.get(target).and_then(|node| node.parent) == Some(parent));
        (parent, after)
    }

    pub fn remove_tile_node(&mut self, id: NodeId) -> Option<Tile<W>> {
        let old_geometries = self.compute_geometry();
        if !matches!(
            self.nodes.get(&id).map(|node| &node.value),
            Some(TreeNode::Leaf { .. })
        ) {
            return None;
        }
        let removed_fullscreen = self.fullscreen_node() == Some(id);
        let node = self.remove_node(id)?;
        if removed_fullscreen {
            self.mapped_under_fullscreen.clear();
            self.moved_under_fullscreen.clear();
        }
        let TreeNode::Leaf { mut tile } = node.value else {
            unreachable!();
        };
        tile.clear_tiled_content_size();
        self.interactive_resize = None;
        if let Some(parent) = node.parent {
            self.remove_child(parent, id);
            self.reap_empty_from(parent);
        }
        if self.windows().next().is_none() {
            let Some(&TreeNode::Split { layout, .. }) =
                self.nodes.get(&self.root).map(|node| &node.value)
            else {
                unreachable!()
            };
            self.empty_representation_layout = Some(layout);
            self.pending_modes.clear();
            self.set_focus_id(None);
        } else if self.focus == Some(id) {
            self.set_focus_id(
                self.fullscreen_node()
                    .and_then(|fullscreen| self.focused_leaf_in(fullscreen))
                    .or_else(|| self.focused_leaf_in(self.root)),
            );
        }
        self.animate_geometry_changes(old_geometries, None);
        Some(*tile)
    }

    /// Removes a window that is being transferred elsewhere rather than closed. Sway refocuses
    /// the most recent focus entry under the old parent, which is a container when that
    /// container was focused on its own (sway/commands/move.c:598-608;
    /// sway/tree/root.c:128-140). Closing uses the view-only rule in `remove_tile`.
    pub fn remove_tile_for_transfer(
        &mut self,
        window: &W::Id,
        transaction: Transaction,
    ) -> Option<Tile<W>> {
        let id = self.node_for_window(window)?;
        let target = (self.focus == Some(id) && self.fullscreen_node().is_none())
            .then(|| {
                self.transfer_focus_target(Some(id), self.nodes.get(&id).and_then(|n| n.parent))
            })
            .flatten();
        let tile = self.remove_tile(window, transaction)?;
        if self.focus.is_some() {
            self.resolve_transfer_focus(target);
        }
        Some(tile)
    }

    pub fn remove_tile(&mut self, window: &W::Id, transaction: Transaction) -> Option<Tile<W>> {
        let id = self.node_for_window(window)?;
        let tile = self.remove_tile_node(id)?;
        self.request_window_sizes_with(Some(transaction), true);
        Some(tile)
    }

    /// As `remove_tile`, but resizes the remaining windows without a transaction or an
    /// animation. Emptied parents are reaped either way.
    pub fn remove_tile_without_transaction(&mut self, window: &W::Id) -> Option<Tile<W>> {
        let id = self.node_for_window(window)?;
        let tile = self.remove_tile_node(id)?;
        self.request_window_sizes();
        Some(tile)
    }

    pub fn add_tile_to_existing_parent(
        &mut self,
        mut tile: Tile<W>,
        parent: NodeId,
        activate: bool,
    ) -> NodeId {
        self.interactive_resize = None;
        tile.update_config(self.view_size, self.scale, self.options.clone());
        let old_geometries = self.compute_geometry();
        let id = self.alloc(Node {
            parent: Some(parent),
            value: TreeNode::Leaf {
                tile: Box::new(tile),
            },
        });
        self.insert_child(parent, id, None);
        if activate {
            self.set_focus_id(Some(id));
        }
        self.animate_geometry_changes(old_geometries, Some(id));
        self.request_window_sizes();
        id
    }
}

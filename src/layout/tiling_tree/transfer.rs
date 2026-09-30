use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn detach_subtree_for_swap(
        &mut self,
        id: NodeId,
    ) -> Option<(DetachedSubtree<W>, DetachedSlot)> {
        if id == self.root {
            return None;
        }
        let parent = self.nodes.get(&id)?.parent?;
        let index = self.child_index(parent, id)?;
        let percent = match &self.nodes.get(&parent)?.value {
            TreeNode::Split { percents, .. } => *percents.get(index)?,
            TreeNode::Leaf { .. } => return None,
        };
        let focus_rank = self
            .focus_history
            .iter()
            .position(|candidate| self.contains_node(id, *candidate))
            .unwrap_or(self.focus_history.len());
        let focused = self
            .focus
            .is_some_and(|focus| self.contains_node(id, focus));
        let (subtree, _) = self.detach_subtree(id)?;
        Some((
            subtree,
            DetachedSlot {
                parent,
                index,
                percent,
                focus_rank,
                focused,
            },
        ))
    }

    pub fn attach_subtree_for_swap(
        &mut self,
        subtree: DetachedSubtree<W>,
        slot: DetachedSlot,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        if !matches!(
            self.nodes.get(&slot.parent).map(|node| &node.value),
            Some(TreeNode::Split { .. })
        ) {
            debug_assert!(false, "detached swap slot parent must remain a split");
            return self.attach_subtree_at(subtree, None);
        }
        let mut remapped = Vec::new();
        let id = self.insert_detached_node(subtree.node, None, &mut remapped);
        let TreeNode::Split {
            children, percents, ..
        } = &mut self
            .nodes
            .get_mut(&slot.parent)
            .expect("invariant: the validated swap slot parent remains in the arena")
            .value
        else {
            unreachable!()
        };
        let index = slot.index.min(children.len());
        for percent in percents.iter_mut() {
            *percent *= 1. - slot.percent;
        }
        children.insert(index, id);
        percents.insert(index, slot.percent);
        self.nodes
            .get_mut(&id)
            .expect("invariant: a freshly inserted detached node remains in the arena")
            .parent = Some(slot.parent);
        for window in subtree.focus_history.into_iter().rev() {
            if let Some(leaf) = self.node_for_window(&window) {
                self.focus_history.retain(|candidate| *candidate != leaf);
                self.focus_history
                    .insert(slot.focus_rank.min(self.focus_history.len()), leaf);
            }
        }
        if slot.focused || self.focus.is_none() {
            self.set_focus_id(self.focused_leaf_in(id));
        }
        self.request_window_sizes();
        (id, remapped)
    }

    pub fn detach_subtree(&mut self, id: NodeId) -> Option<(DetachedSubtree<W>, Option<NodeId>)> {
        if !self.nodes.contains_key(&id) {
            return None;
        }
        self.interactive_resize = None;
        let root_focused = self.focus == Some(id);
        let leaves = self.leaf_ids_in(id);
        let focus_history = self
            .focus_history
            .iter()
            .filter(|candidate| leaves.contains(candidate))
            .filter_map(|leaf| self.tile(*leaf).map(|tile| tile.window().id().clone()))
            .collect();
        let parent = if id == self.root {
            None
        } else {
            let parent = self.detach_subtree_only(id)?;
            Some(parent)
        };
        let moved_fullscreen = self.fullscreen_node() == Some(id);
        let node = if id == self.root {
            let TreeNode::Split {
                layout,
                children,
                percents,
            } = std::mem::replace(
                &mut self.nodes.get_mut(&self.root)?.value,
                TreeNode::Split {
                    layout: Layout::SplitH,
                    children: Vec::new(),
                    percents: Vec::new(),
                },
            )
            else {
                return None;
            };
            self.empty_representation_layout = Some(Layout::SplitH);
            let children = children
                .into_iter()
                .map(|child| self.take_detached_node(child))
                .collect::<Option<Vec<_>>>()?;
            // The emptied root stays behind with its ID, like sway's workspace
            // keeping its identity while `workspace_wrap_children` creates a new
            // container (`sway/tree/workspace.c:898-910`). Handing the root's ID
            // to the detached split would leave the ID live in both trees.
            DetachedNode::Split {
                old_id: NodeId(NODE_ID_COUNTER.next()),
                layout,
                children,
                percents,
                previous_layout: self.previous_split_layouts.remove(&id),
                title_format: self.title_formats.remove(&id),
                pending_mode: self.pending_modes.remove(&id),
                sticky: self.sticky_splits.remove(&id),
            }
        } else {
            self.take_detached_node(id)?
        };
        self.fullscreen_tile_slot = false;
        if moved_fullscreen {
            self.mapped_under_fullscreen.clear();
            self.fullscreen_layout_wrappers.clear();
            self.pre_layout_ipc_rects.clear();
        }
        self.focus = self.focused_leaf_in(self.root);
        self.request_window_sizes();
        Some((
            DetachedSubtree {
                node,
                focus_history,
                root_focused,
            },
            parent,
        ))
    }

    pub fn attach_subtree(
        &mut self,
        subtree: DetachedSubtree<W>,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        let target = self.focus;
        self.attach_subtree_at(subtree, target)
    }

    pub fn attach_subtree_at(
        &mut self,
        subtree: DetachedSubtree<W>,
        target: Option<NodeId>,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        if subtree.has_fullscreen() {
            if let Some(current) = self.fullscreen_node() {
                self.replace_fullscreen_state(current, None);
            }
        }
        let focus_history = subtree.focus_history;
        let mut remapped = Vec::new();
        let node = if self.is_empty() {
            match subtree.node {
                DetachedNode::Split {
                    old_id,
                    layout,
                    children,
                    percents: detached_percents,
                    previous_layout,
                    title_format,
                    pending_mode,
                    sticky,
                } => {
                    self.attach_split_to_empty_root(
                        DetachedSplit {
                            old_id,
                            layout,
                            children,
                            percents: detached_percents,
                            previous_layout,
                            title_format,
                            pending_mode,
                            sticky,
                        },
                        focus_history,
                        &mut remapped,
                    );
                    return (self.root, remapped);
                }
                node => node,
            }
        } else {
            subtree.node
        };
        let height = node.height();
        let id = self.insert_detached_node(node, None, &mut remapped);
        let (parent, after) =
            match target.and_then(|target| self.nodes.get(&target).map(|node| (target, node))) {
                Some((
                    target,
                    Node {
                        parent: Some(parent),
                        value: TreeNode::Leaf { .. },
                    },
                )) => (*parent, Some(target)),
                Some((
                    target,
                    Node {
                        value: TreeNode::Split { .. },
                        ..
                    },
                )) => (target, None),
                _ => (self.root, None),
            };
        // Placing a deep subtree under a deep target could exceed the depth
        // bound; the workspace root always has room, because the subtree came
        // from a tree that respected it.
        let (parent, after) = if self.fits_below(parent, height) {
            (parent, after)
        } else {
            (self.root, None)
        };
        self.insert_child(parent, id, after);
        self.restore_transferred_focus(focus_history);
        if self.focus.is_none() {
            self.set_focus_id(self.focused_leaf_in(id));
        }
        self.request_window_sizes();
        (id, remapped)
    }

    fn attach_split_to_empty_root(
        &mut self,
        split: DetachedSplit<W>,
        focus_history: Vec<W::Id>,
        remapped: &mut Vec<(NodeId, NodeId)>,
    ) {
        let DetachedSplit {
            old_id,
            layout,
            children,
            percents: detached_percents,
            previous_layout,
            title_format,
            pending_mode,
            sticky,
        } = split;
        if old_id != self.root {
            remapped.push((old_id, self.root));
        }
        let TreeNode::Split {
            layout: root_layout,
            ..
        } = &mut self.nodes.get_mut(&self.root).unwrap().value
        else {
            unreachable!();
        };
        *root_layout = layout;
        if let Some(layout) = previous_layout {
            self.previous_split_layouts.insert(self.root, layout);
        }
        if let Some(format) = title_format {
            self.title_formats.insert(self.root, format);
        }
        if sticky {
            self.sticky_splits.insert(self.root);
        }
        if let Some(mode) = pending_mode {
            self.pending_modes.insert(self.root, mode);
        }
        let ids = children
            .into_iter()
            .map(|child| self.insert_detached_node(child, Some(self.root), remapped))
            .collect::<Vec<_>>();
        let TreeNode::Split {
            children, percents, ..
        } = &mut self.nodes.get_mut(&self.root).unwrap().value
        else {
            unreachable!();
        };
        *percents = detached_percents;
        *children = ids;
        self.has_had_tile = true;
        self.restore_transferred_focus(focus_history);
        self.request_window_sizes();
    }

    pub(super) fn restore_transferred_focus(&mut self, focus_history: Vec<W::Id>) {
        let insertion = usize::from(self.focus.is_some());
        for window in focus_history.into_iter().rev() {
            if let Some(leaf) = self.node_for_window(&window) {
                self.focus_history.retain(|candidate| *candidate != leaf);
                self.focus_history
                    .insert(insertion.min(self.focus_history.len()), leaf);
            }
        }
        if self.focus.is_none() {
            self.set_focus_id(self.focused_leaf_in(self.root));
        }
    }

    pub fn finish_subtree_detach(&mut self, old_parent: Option<NodeId>) {
        if let Some(parent) = old_parent {
            self.reap_empty_from(parent);
        }
        self.compact_tree();
        self.focus = self.focused_leaf_in(self.root);
        self.request_window_sizes();
    }

    pub(super) fn take_detached_node(&mut self, id: NodeId) -> Option<DetachedNode<W>> {
        let previous_layout = self.previous_split_layouts.get(&id).copied();
        let title_format = self.title_formats.get(&id).cloned();
        let pending_mode = self.pending_modes.get(&id).copied();
        let sticky = self.sticky_splits.contains(&id);
        let mapped_under_fullscreen = self.mapped_under_fullscreen.contains(&id);
        let node = self.remove_node(id)?;
        match node.value {
            TreeNode::Split {
                layout,
                children,
                percents,
            } => Some(DetachedNode::Split {
                old_id: id,
                layout,
                children: children
                    .into_iter()
                    .map(|child| self.take_detached_node(child))
                    .collect::<Option<Vec<_>>>()?,
                percents,
                previous_layout,
                title_format,
                pending_mode,
                sticky,
            }),
            TreeNode::Leaf { tile } => Some(DetachedNode::Leaf {
                old_id: id,
                tile,
                pending_mode,
                mapped_under_fullscreen,
            }),
        }
    }

    pub(super) fn insert_detached_node(
        &mut self,
        node: DetachedNode<W>,
        parent: Option<NodeId>,
        remapped: &mut Vec<(NodeId, NodeId)>,
    ) -> NodeId {
        match node {
            DetachedNode::Split {
                old_id,
                layout,
                children,
                percents,
                previous_layout,
                title_format,
                pending_mode,
                sticky,
            } => self.insert_detached_split(
                DetachedSplit {
                    old_id,
                    layout,
                    children,
                    percents,
                    previous_layout,
                    title_format,
                    pending_mode,
                    sticky,
                },
                parent,
                remapped,
            ),
            DetachedNode::Leaf {
                old_id,
                mut tile,
                pending_mode,
                mapped_under_fullscreen,
            } => {
                tile.update_config(self.view_size, self.scale, self.options.clone());
                let id = self.insert_with_id(
                    old_id,
                    Node {
                        parent,
                        value: TreeNode::Leaf { tile },
                    },
                );
                if id != old_id {
                    remapped.push((old_id, id));
                }
                if let Some(mode) = pending_mode {
                    self.pending_modes.insert(id, mode);
                }
                if mapped_under_fullscreen {
                    self.mapped_under_fullscreen.insert(id);
                }
                id
            }
        }
    }

    fn insert_detached_split(
        &mut self,
        split: DetachedSplit<W>,
        parent: Option<NodeId>,
        remapped: &mut Vec<(NodeId, NodeId)>,
    ) -> NodeId {
        let DetachedSplit {
            old_id,
            layout,
            children,
            percents,
            previous_layout,
            title_format,
            pending_mode,
            sticky,
        } = split;
        let id = self.insert_with_id(
            old_id,
            Node {
                parent,
                value: TreeNode::Split {
                    layout,
                    children: Vec::new(),
                    percents,
                },
            },
        );
        if id != old_id {
            remapped.push((old_id, id));
        }
        let children = children
            .into_iter()
            .map(|child| self.insert_detached_node(child, Some(id), remapped))
            .collect();
        let TreeNode::Split { children: slot, .. } = &mut self
            .nodes
            .get_mut(&id)
            .expect("invariant: a freshly allocated split remains in the arena")
            .value
        else {
            unreachable!();
        };
        *slot = children;
        if let Some(layout) = previous_layout {
            self.previous_split_layouts.insert(id, layout);
        }
        if let Some(format) = title_format {
            self.title_formats.insert(id, format);
        }
        if sticky {
            self.sticky_splits.insert(id);
        }
        if let Some(mode) = pending_mode {
            self.pending_modes.insert(id, mode);
        }
        id
    }
}

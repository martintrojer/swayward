use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn split(&mut self, id: NodeId, layout: Layout) {
        self.interactive_resize = None;
        if id == self.root && self.split_len(id).is_some_and(|len| len > 0) {
            let old_layout = match self.nodes[&id].value {
                TreeNode::Split { layout, .. } => layout,
                TreeNode::Leaf { .. } => unreachable!(),
            };
            let wrapper = self.wrap_root_children(old_layout);
            if let TreeNode::Split {
                layout: root_layout,
                ..
            } = &mut self.nodes.get_mut(&id).unwrap().value
            {
                *root_layout = layout;
            }
            self.set_focus_id(Some(wrapper));
            self.request_window_sizes();
            return;
        }
        if matches!(
            self.nodes.get(&id).map(|node| &node.value),
            Some(TreeNode::Leaf { .. })
        ) {
            let singleton_split_parent =
                self.nodes
                    .get(&id)
                    .and_then(|node| node.parent)
                    .filter(|parent| {
                        matches!(
                            self.nodes.get(parent).map(|node| &node.value),
                            Some(TreeNode::Split {
                                layout: Layout::SplitH | Layout::SplitV,
                                children,
                                ..
                            }) if children.len() == 1
                        )
                    });
            if let Some(parent) = singleton_split_parent {
                if let Some(Node {
                    value:
                        TreeNode::Split {
                            layout: current, ..
                        },
                    ..
                }) = self.nodes.get_mut(&parent)
                {
                    *current = layout;
                }
            } else {
                self.wrap_node(id, layout);
            }
        } else {
            let parent = self.nodes[&id].parent.unwrap_or(self.root);
            let siblings = self.split_len(parent).unwrap_or_default();
            if id == self.root {
                if let Some(Node {
                    value:
                        TreeNode::Split {
                            layout: current, ..
                        },
                    ..
                }) = self.nodes.get_mut(&id)
                {
                    *current = layout;
                }
            } else if siblings <= 1 {
                if let Some(Node {
                    value:
                        TreeNode::Split {
                            layout: current, ..
                        },
                    ..
                }) = self.nodes.get_mut(&parent)
                {
                    *current = layout;
                }
            } else {
                let index = self.child_index(parent, id).unwrap();
                let old_percent = match &self.nodes[&parent].value {
                    TreeNode::Split { percents, .. } => percents[index],
                    TreeNode::Leaf { .. } => unreachable!(),
                };
                let wrapper = self.alloc(Node {
                    parent: Some(parent),
                    value: TreeNode::Split {
                        layout,
                        children: vec![id],
                        percents: vec![1.],
                    },
                });
                let TreeNode::Split {
                    children, percents, ..
                } = &mut self.nodes.get_mut(&parent).unwrap().value
                else {
                    unreachable!();
                };
                children[index] = wrapper;
                percents[index] = old_percent;
                self.nodes.get_mut(&id).unwrap().parent = Some(wrapper);
            }
            self.compact_tree();
            self.request_window_sizes();
        }
    }

    pub fn set_layout(&mut self, id: NodeId, layout: Layout) {
        self.interactive_resize = None;
        if let Some(Node {
            value: TreeNode::Split {
                layout: current, ..
            },
            ..
        }) = self.nodes.get_mut(&id)
        {
            if matches!(*current, Layout::SplitH | Layout::SplitV) && *current != layout {
                self.previous_split_layouts.insert(id, *current);
            }
            *current = layout;
            self.compact_tree();
            self.request_window_sizes();
        } else {
            self.split(id, layout);
        }
    }

    pub fn toggle_focused_tabbed_display(&mut self) {
        let Some(parent) = self.focus.and_then(|id| self.nodes.get(&id)?.parent) else {
            return;
        };
        let layout = match &self.nodes.get(&parent).map(|node| &node.value) {
            Some(TreeNode::Split {
                layout: Layout::Tabbed,
                ..
            }) => Layout::SplitH,
            _ => Layout::Tabbed,
        };
        self.set_layout(parent, layout);
    }

    pub fn set_focused_layout(&mut self, layout: Layout) -> Vec<(NodeId, NodeId)> {
        // Sway starts exposing the workspace representation after an explicit
        // layout command, even when the workspace has never held a window.
        self.has_had_tile = true;
        let focus = self.focus;
        let (target, remapped) = self.focused_layout_target();
        let Some(target) = target else {
            self.set_layout(self.root, layout);
            return remapped;
        };
        let grouped_root = matches!(
            self.nodes.get(&self.root).map(|node| &node.value),
            Some(TreeNode::Split {
                layout: Layout::Tabbed | Layout::Stacked,
                ..
            })
        );
        if target == self.root
            && focus.is_some_and(|focus| self.tile(focus).is_some() || grouped_root)
            && matches!(layout, Layout::Tabbed | Layout::Stacked)
        {
            self.wrap_root_children(layout);
            self.request_window_sizes();
        } else {
            self.set_layout_for_command(target, layout);
        }
        remapped
    }

    pub fn split_focused(&mut self, layout: Layout) {
        if let Some(focus) = self.focus {
            self.split(focus, layout);
        } else {
            self.set_layout(self.root, layout);
        }
    }

    pub fn flatten_parent(&mut self, id: NodeId) -> Option<(NodeId, NodeId)> {
        self.interactive_resize = None;
        let parent = self.nodes.get(&id)?.parent?;
        if parent == self.root || self.split_len(parent) != Some(1) {
            return None;
        }
        let grandparent = self.nodes.get(&parent)?.parent?;
        let TreeNode::Split { children, .. } = &mut self.nodes.get_mut(&grandparent)?.value else {
            return None;
        };
        let index = children.iter().position(|child| *child == parent)?;
        children[index] = id;
        self.nodes.get_mut(&id)?.parent = Some(grandparent);
        if let Some(fullscreen) = self
            .pending_modes
            .get(&parent)
            .and_then(|mode| mode.fullscreen)
        {
            self.pending_modes
                .entry(id)
                .or_insert(PendingMode {
                    fullscreen: None,
                    maximized: false,
                })
                .fullscreen = Some(fullscreen);
        }
        if self.focus == Some(parent) {
            self.set_focus_id(Some(id));
        }
        self.remove_node(parent);
        self.request_window_sizes();
        Some((parent, id))
    }

    pub fn toggle_split(&mut self, id: NodeId) {
        let layout = self
            .nodes
            .get(&id)
            .and_then(|node| node.parent)
            .and_then(|parent| self.nodes.get(&parent))
            .and_then(|parent| match parent.value {
                TreeNode::Split { layout, .. } => Some(layout),
                TreeNode::Leaf { .. } => None,
            });
        self.split(
            id,
            if layout == Some(Layout::SplitV) {
                Layout::SplitH
            } else {
                Layout::SplitV
            },
        );
    }

    pub fn toggle_focused_layout(&mut self, toggle: &LayoutToggle) -> Vec<(NodeId, NodeId)> {
        let (target, remapped) = self.focused_layout_target();
        if let Some(target) = target {
            self.toggle_node_layout(target, toggle);
        }
        remapped
    }

    pub fn toggle_target_layout(&mut self, id: NodeId, toggle: &LayoutToggle) -> bool {
        let Some(target) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return false;
        };
        self.toggle_node_layout(target, toggle)
    }

    pub fn toggle_node_layout(&mut self, target: NodeId, toggle: &LayoutToggle) -> bool {
        let current = match self.nodes.get(&target).map(|node| &node.value) {
            Some(TreeNode::Split { layout, .. }) => *layout,
            Some(TreeNode::Leaf { .. }) | None => return false,
        };
        let tree_layout = |layout| match layout {
            swayward_ipc::command::Layout::SplitH => Some(Layout::SplitH),
            swayward_ipc::command::Layout::SplitV => Some(Layout::SplitV),
            swayward_ipc::command::Layout::Tabbed => Some(Layout::Tabbed),
            swayward_ipc::command::Layout::Stacked => Some(Layout::Stacked),
            swayward_ipc::command::Layout::ToggleSplit => None,
        };
        let next = match toggle {
            LayoutToggle::Default => match current {
                Layout::SplitH | Layout::SplitV => Layout::Stacked,
                Layout::Stacked => Layout::Tabbed,
                Layout::Tabbed => self
                    .previous_split_layouts
                    .get(&target)
                    .copied()
                    .unwrap_or(Layout::SplitH),
            },
            LayoutToggle::Split => {
                self.toggle_layout_split(target);
                return true;
            }
            LayoutToggle::All => match current {
                Layout::SplitH => Layout::SplitV,
                Layout::SplitV => Layout::Stacked,
                Layout::Stacked => Layout::Tabbed,
                Layout::Tabbed => Layout::SplitH,
            },
            LayoutToggle::Cycle(cycle) => {
                let next = cycle
                    .iter()
                    .position(|candidate| match candidate {
                        LayoutToggleEntry::Split => {
                            matches!(current, Layout::SplitH | Layout::SplitV)
                        }
                        LayoutToggleEntry::Layout(layout) => tree_layout(*layout) == Some(current),
                    })
                    .and_then(|index| cycle.get((index + 1) % cycle.len()))
                    .or_else(|| {
                        cycle
                            .iter()
                            .find(|candidate| matches!(candidate, LayoutToggleEntry::Layout(_)))
                    });
                match next {
                    Some(LayoutToggleEntry::Split) => {
                        self.toggle_layout_split(target);
                        return true;
                    }
                    Some(LayoutToggleEntry::Layout(layout)) => {
                        let Some(layout) = tree_layout(*layout) else {
                            return false;
                        };
                        layout
                    }
                    None => return false,
                }
            }
        };
        self.set_layout_for_command(target, next);
        true
    }

    pub fn restore_focused_split_layout(&mut self) -> Vec<(NodeId, NodeId)> {
        let (target, remapped) = self.focused_layout_target();
        let Some(target) = target else {
            return remapped;
        };
        self.restore_node_layout(target);
        remapped
    }

    pub fn restore_target_layout(&mut self, id: NodeId) -> bool {
        let Some(target) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return false;
        };
        self.restore_node_layout(target)
    }

    pub fn restore_node_layout(&mut self, target: NodeId) -> bool {
        let Some(layout) = self.previous_split_layouts.get(&target).copied() else {
            return self.nodes.contains_key(&target);
        };
        self.set_layout_for_command(target, layout);
        true
    }

    pub fn toggle_focused_layout_split(&mut self) -> Vec<(NodeId, NodeId)> {
        let (target, remapped) = self.focused_layout_target();
        if let Some(target) = target {
            self.toggle_layout_split(target);
        }
        remapped
    }

    fn toggle_layout_split(&mut self, target: NodeId) {
        let layout = match self.nodes.get(&target).map(|node| &node.value) {
            Some(TreeNode::Split {
                layout: Layout::SplitH,
                ..
            }) => Layout::SplitV,
            Some(TreeNode::Split {
                layout: Layout::SplitV,
                ..
            }) => Layout::SplitH,
            _ => self
                .previous_split_layouts
                .get(&target)
                .copied()
                .unwrap_or(Layout::SplitH),
        };
        self.set_layout_for_command(target, layout);
    }

    pub fn toggle_focused_split(&mut self) {
        let Some(focus) = self.focus else { return };
        let layout = match self.nodes.get(&focus).map(|node| &node.value) {
            Some(TreeNode::Split {
                layout: Layout::SplitH,
                ..
            }) => Layout::SplitV,
            _ => Layout::SplitH,
        };
        self.split(focus, layout);
    }

    pub fn set_focused_display(&mut self, display: ColumnDisplay) {
        let Some(parent) = self.focus.and_then(|id| self.nodes.get(&id)?.parent) else {
            return;
        };
        self.set_layout(
            parent,
            if display == ColumnDisplay::Tabbed {
                Layout::Tabbed
            } else {
                Layout::SplitV
            },
        );
    }

    pub(super) fn wrap_node(&mut self, id: NodeId, layout: Layout) -> NodeId {
        let parent = self.nodes[&id].parent.unwrap_or(self.root);
        let index = self.child_index(parent, id).unwrap();
        let old_percent = match &self.nodes[&parent].value {
            TreeNode::Split { percents, .. } => percents[index],
            TreeNode::Leaf { .. } => unreachable!(),
        };
        let wrapper = self.alloc(Node {
            parent: Some(parent),
            value: TreeNode::Split {
                layout,
                children: vec![id],
                percents: vec![1.],
            },
        });
        let TreeNode::Split {
            children, percents, ..
        } = &mut self.nodes.get_mut(&parent).unwrap().value
        else {
            unreachable!();
        };
        children[index] = wrapper;
        percents[index] = old_percent;
        self.nodes.get_mut(&id).unwrap().parent = Some(wrapper);
        wrapper
    }

    fn wrap_root_children(&mut self, layout: Layout) -> NodeId {
        let TreeNode::Split {
            layout: root_layout,
            children,
            percents,
        } = std::mem::replace(
            &mut self.nodes.get_mut(&self.root).unwrap().value,
            TreeNode::Split {
                layout: Layout::SplitH,
                children: Vec::new(),
                percents: Vec::new(),
            },
        )
        else {
            unreachable!();
        };
        let wrapper = self.alloc(Node {
            parent: Some(self.root),
            value: TreeNode::Split {
                layout,
                children,
                percents,
            },
        });
        if matches!(root_layout, Layout::SplitH | Layout::SplitV) {
            self.previous_split_layouts.insert(wrapper, root_layout);
        }
        let children = match &self.nodes[&wrapper].value {
            TreeNode::Split { children, .. } => children.clone(),
            TreeNode::Leaf { .. } => unreachable!(),
        };
        for child in children {
            self.nodes.get_mut(&child).unwrap().parent = Some(wrapper);
        }
        let TreeNode::Split {
            layout: layout_slot,
            children,
            percents,
        } = &mut self.nodes.get_mut(&self.root).unwrap().value
        else {
            unreachable!();
        };
        *layout_slot = root_layout;
        *children = vec![wrapper];
        *percents = vec![1.];
        wrapper
    }

    pub fn set_target_layout(&mut self, id: NodeId, layout: Layout) -> bool {
        if !self.nodes.contains_key(&id) {
            return false;
        }
        let target = self.nodes[&id].parent.unwrap_or(self.root);
        self.set_layout_for_command(target, layout);
        true
    }

    // Unlike general tree compaction, sway's `layout` command flattens at most one ancestor.
    fn set_layout_for_command(&mut self, id: NodeId, layout: Layout) {
        self.interactive_resize = None;
        if let Some(Node {
            value: TreeNode::Split {
                layout: current, ..
            },
            ..
        }) = self.nodes.get_mut(&id)
        {
            if matches!(*current, Layout::SplitH | Layout::SplitV) && *current != layout {
                self.previous_split_layouts.insert(id, *current);
            }
            *current = layout;
            self.request_window_sizes();
        }
    }

    // Sway operates on the focused container's parent. When both that parent and its parent are
    // singletons, it replaces the parent with its child once and operates on the grandparent.
    fn focused_layout_target(&mut self) -> (Option<NodeId>, Vec<(NodeId, NodeId)>) {
        let Some(focus) = self.focus else {
            return (None, Vec::new());
        };
        let target = if focus == self.root {
            self.root
        } else {
            let Some(node) = self.nodes.get(&focus) else {
                return (None, Vec::new());
            };
            node.parent.unwrap_or(self.root)
        };
        if target == self.root || self.split_len(target) != Some(1) {
            return (Some(target), Vec::new());
        }
        let Some(grandparent) = self.nodes.get(&target).and_then(|node| node.parent) else {
            return (Some(target), Vec::new());
        };
        if grandparent == self.root || self.split_len(grandparent) != Some(1) {
            return (Some(target), Vec::new());
        }
        let child = match &self.nodes.get(&target).unwrap().value {
            TreeNode::Split { children, .. } => children[0],
            TreeNode::Leaf { .. } => return (Some(target), Vec::new()),
        };
        let Some(remapped) = self.flatten_parent(child) else {
            return (Some(target), Vec::new());
        };
        (Some(grandparent), vec![remapped])
    }
}

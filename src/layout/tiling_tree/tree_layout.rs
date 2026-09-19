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

    pub fn toggle_column_tabbed_display(&mut self) {
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

    pub fn set_focused_layout(&mut self, layout: Layout) {
        let focus = self.focus;
        let Some(target) = self.focused_layout_target() else {
            self.set_layout(self.root, layout);
            return;
        };
        if target == self.root
            && focus.is_some_and(|focus| self.tile(focus).is_some())
            && matches!(layout, Layout::Tabbed | Layout::Stacked)
        {
            self.wrap_root_children(layout);
            self.request_window_sizes();
        } else {
            self.set_layout_for_command(target, layout);
        }
    }

    pub fn split_focused(&mut self, layout: Layout) {
        if let Some(focus) = self.focus {
            self.split(focus, layout);
        } else {
            self.set_layout(self.root, layout);
        }
    }

    pub fn toggle_focused_layout(&mut self, toggle: &LayoutToggle) {
        let Some(target) = self.focused_layout_target() else {
            return;
        };
        let current = match self.nodes.get(&target).map(|node| &node.value) {
            Some(TreeNode::Split { layout, .. }) => *layout,
            Some(TreeNode::Leaf { .. }) | None => return,
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
                return;
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
                        return;
                    }
                    Some(LayoutToggleEntry::Layout(layout)) => {
                        let Some(layout) = tree_layout(*layout) else {
                            return;
                        };
                        layout
                    }
                    None => return,
                }
            }
        };
        self.set_layout_for_command(target, next);
    }

    pub fn restore_focused_split_layout(&mut self) {
        let Some(target) = self.focused_layout_target() else {
            return;
        };
        let Some(layout) = self.previous_split_layouts.get(&target).copied() else {
            return;
        };
        self.set_layout_for_command(target, layout);
    }

    pub fn toggle_focused_layout_split(&mut self) {
        let Some(target) = self.focused_layout_target() else {
            return;
        };
        self.toggle_layout_split(target);
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

    pub fn set_column_display(&mut self, display: ColumnDisplay) {
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
    fn focused_layout_target(&mut self) -> Option<NodeId> {
        let focus = self.focus?;
        let target = if focus == self.root {
            self.root
        } else {
            self.nodes.get(&focus)?.parent.unwrap_or(self.root)
        };
        if target == self.root || self.split_len(target) != Some(1) {
            return Some(target);
        }
        let grandparent = self.nodes.get(&target)?.parent?;
        if grandparent == self.root || self.split_len(grandparent) != Some(1) {
            return Some(target);
        }
        let child = match &self.nodes.get(&target)?.value {
            TreeNode::Split { children, .. } => children[0],
            TreeNode::Leaf { .. } => return Some(target),
        };
        let TreeNode::Split { children, .. } = &mut self.nodes.get_mut(&grandparent)?.value else {
            return Some(target);
        };
        children[0] = child;
        self.nodes.get_mut(&child)?.parent = Some(grandparent);
        self.remove_node(target);
        Some(grandparent)
    }
}

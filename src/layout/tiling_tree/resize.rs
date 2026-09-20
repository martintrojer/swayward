use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn resize_adjacent(&mut self, first: NodeId, second: NodeId, delta: f64) -> bool {
        let old = self.compute_geometry();
        let changed = self.resize_adjacent_inner(first, second, delta);
        if changed {
            self.animate_geometry_changes(old, None);
        }
        changed
    }

    fn resize_adjacent_inner(&mut self, first: NodeId, second: NodeId, delta: f64) -> bool {
        if !delta.is_finite() {
            return false;
        }
        let Some(parent) = self.nodes.get(&first).and_then(|node| node.parent) else {
            return false;
        };
        if self.nodes.get(&second).and_then(|node| node.parent) != Some(parent) {
            return false;
        }
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return false;
        };
        let Some(first_index) = children.iter().position(|child| *child == first) else {
            return false;
        };
        let Some(second_index) = children.iter().position(|child| *child == second) else {
            return false;
        };
        if first_index.abs_diff(second_index) != 1 {
            return false;
        }
        let first_percent = percents[first_index] + delta;
        let second_percent = percents[second_index] - delta;
        if first_percent <= 0. || second_percent <= 0. {
            return false;
        }
        percents[first_index] = first_percent;
        percents[second_index] = second_percent;
        self.request_window_sizes();
        true
    }

    pub fn center_column(&mut self) {}
    pub fn center_window(&mut self, _window: Option<&W::Id>) {}
    pub fn center_visible_columns(&mut self) {}

    pub fn toggle_width(&mut self, forwards: bool) {
        self.toggle_window_width(None, forwards);
    }

    pub fn toggle_full_width(&mut self) {
        let Some(id) = self.focus else { return };
        let Some(rect) = self.geometry(id) else {
            return;
        };
        self.resize_node_dimension(
            id,
            true,
            SizeChange::AdjustFixed((self.view_size.w - rect.size.w) as i32),
        );
    }

    pub fn set_window_border(
        &mut self,
        window: &W::Id,
        style: swayward_ipc::command::BorderStyle,
        width: Option<u16>,
    ) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        let Some(tile) = self.tile_mut(id) else {
            return false;
        };
        if tile.set_sway_border(style, width, false).is_err() {
            return false;
        }
        self.request_window_sizes();
        true
    }

    pub fn set_window_width(&mut self, window: Option<&W::Id>, change: SizeChange) {
        if let Some(id) = self.resolve_node(window) {
            self.resize_node_dimension(id, true, change);
        }
    }

    pub fn set_window_height(&mut self, window: Option<&W::Id>, change: SizeChange) {
        if let Some(id) = self.resolve_node(window) {
            self.resize_node_dimension(id, false, change);
        }
    }

    pub fn set_window_size_sway(
        &mut self,
        window: &W::Id,
        width: Option<SizeChange>,
        height: Option<SizeChange>,
    ) {
        let Some(id) = self.node_for_window(window) else {
            return;
        };
        self.set_node_size_sway(id, width, height);
    }

    pub fn set_node_size_sway(
        &mut self,
        id: NodeId,
        width: Option<SizeChange>,
        height: Option<SizeChange>,
    ) {
        if let Some(change) = width {
            self.resize_node_dimension_sway(id, true, change);
        }
        if let Some(change) = height {
            self.resize_node_dimension_sway(id, false, change);
        }
    }

    pub fn resize_window_edge(
        &mut self,
        window: Option<&W::Id>,
        edge: ResizeEdge,
        change: SizeChange,
    ) -> bool {
        let Some(id) = self.resolve_node(window) else {
            return false;
        };
        let horizontal = edge.intersects(ResizeEdge::LEFT_RIGHT);
        let layout = if horizontal {
            Layout::SplitH
        } else {
            Layout::SplitV
        };
        let before = edge.intersects(ResizeEdge::LEFT | ResizeEdge::TOP);
        let Some((first, second, _, _, axis_size, _)) = self.resize_boundary(id, layout, before)
        else {
            return false;
        };
        let delta = match change {
            SizeChange::AdjustFixed(value) => f64::from(value) / axis_size.max(1.),
            SizeChange::AdjustProportion(value) => value / 100.,
            SizeChange::SetFixed(_) | SizeChange::SetProportion(_) => return false,
        };
        self.resize_adjacent(first, second, delta)
    }

    pub fn reset_window_height(&mut self, window: Option<&W::Id>) {
        let Some(id) = self.resolve_node(window) else {
            return;
        };
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return;
        };
        if let Some(Node {
            value:
                TreeNode::Split {
                    layout: Layout::SplitV,
                    children,
                    percents,
                },
            ..
        }) = self.nodes.get_mut(&parent)
        {
            percents.fill(1. / children.len() as f64);
            self.request_window_sizes();
        }
    }

    pub fn toggle_window_width(&mut self, window: Option<&W::Id>, forwards: bool) {
        self.toggle_preset(window, true, forwards);
    }

    pub fn toggle_window_height(&mut self, window: Option<&W::Id>, forwards: bool) {
        self.toggle_preset(window, false, forwards);
    }

    pub fn expand_column_to_available_width(&mut self) {
        self.toggle_full_width();
    }

    pub fn interactive_resize_begin(&mut self, window: W::Id, edges: ResizeEdge) -> bool {
        if self.interactive_resize.is_some() {
            return false;
        }
        let Some(id) = self.node_for_window(&window) else {
            return false;
        };
        if self
            .pending_modes
            .get(&id)
            .is_some_and(|mode| mode.fullscreen.is_some() || mode.maximized)
        {
            return false;
        }
        let horizontal = edges.intersects(ResizeEdge::LEFT_RIGHT);
        let vertical = edges.intersects(ResizeEdge::TOP_BOTTOM);
        let wanted_layout = if horizontal {
            Layout::SplitH
        } else if vertical {
            Layout::SplitV
        } else {
            return false;
        };
        let toward_before = edges.intersects(ResizeEdge::LEFT | ResizeEdge::TOP);
        let Some((first, second, first_percent, second_percent, axis_size, sign)) =
            self.resize_boundary(id, wanted_layout, toward_before)
        else {
            return false;
        };
        self.interactive_resize = Some(InteractiveResize {
            window,
            target: id,
            first,
            second,
            initial_first: first_percent,
            initial_second: second_percent,
            axis_size,
            sign,
            data: InteractiveResizeData { edges },
        });
        true
    }

    pub fn interactive_resize_update(
        &mut self,
        window: &W::Id,
        delta: Point<f64, Logical>,
    ) -> bool {
        let Some(resize) = &self.interactive_resize else {
            return false;
        };
        if &resize.window != window {
            return false;
        }
        let amount = if resize.data.edges.intersects(ResizeEdge::LEFT_RIGHT) {
            delta.x
        } else {
            delta.y
        } * resize.sign
            / resize.axis_size.max(1.);
        let (first, second) = (resize.first, resize.second);
        let current = self.sibling_percents(first, second);
        let Some((current_first, current_second)) = current else {
            return false;
        };
        let target_first = resize.initial_first + amount;
        let target_second = resize.initial_second - amount;
        let change = target_first - current_first;
        if target_first <= 0. || target_second <= 0. {
            return false;
        }
        let changed = self.resize_adjacent(first, second, change);
        debug_assert!((current_second - change - target_second).abs() <= 1e-6);
        changed
    }

    pub fn interactive_resize_end(&mut self, window: Option<&W::Id>) {
        if window.is_none_or(|window| {
            self.interactive_resize
                .as_ref()
                .is_some_and(|resize| &resize.window == window)
        }) {
            self.interactive_resize = None;
        }
    }

    fn resolve_node(&self, window: Option<&W::Id>) -> Option<NodeId> {
        window
            .and_then(|window| self.node_for_window(window))
            .or(self.focus)
    }

    fn toggle_preset(&mut self, window: Option<&W::Id>, width: bool, forwards: bool) {
        let presets = if width {
            &self.options.layout.preset_column_widths
        } else {
            &self.options.layout.preset_window_heights
        };
        if presets.is_empty() {
            return;
        }
        let index = if forwards { 0 } else { presets.len() - 1 };
        let change = match presets[index] {
            PresetSize::Fixed(value) => SizeChange::SetFixed(value),
            PresetSize::Proportion(value) => SizeChange::SetProportion(value * 100.),
        };
        if width {
            self.set_window_width(window, change);
        } else {
            self.set_window_height(window, change);
        }
    }

    fn resize_node_dimension(&mut self, id: NodeId, width: bool, change: SizeChange) {
        let wanted = if width {
            Layout::SplitH
        } else {
            Layout::SplitV
        };
        let Some(rect) = self.geometry(id) else {
            return;
        };
        let current = if width { rect.size.w } else { rect.size.h };
        let total = if width {
            self.view_size.w
        } else {
            self.view_size.h
        };
        let mut branch = id;
        let mut parent = self.nodes.get(&id).and_then(|node| node.parent);
        while let Some(parent_id) = parent {
            let Some(Node {
                parent: grandparent,
                value: TreeNode::Split {
                    layout, children, ..
                },
            }) = self.nodes.get(&parent_id)
            else {
                return;
            };
            if *layout == wanted && children.len() > 1 {
                let extent = self
                    .node_geometry(parent_id)
                    .map(|rect| if width { rect.size.w } else { rect.size.h })
                    .unwrap_or(total)
                    .max(1.);
                let (delta, across_all_siblings) = match change {
                    SizeChange::AdjustFixed(value) => (f64::from(value) / extent, true),
                    SizeChange::AdjustProportion(value) => (value / 100., true),
                    SizeChange::SetFixed(value) => ((f64::from(value) - current) / extent, false),
                    SizeChange::SetProportion(value) => {
                        ((total * value / 100. - current) / extent, false)
                    }
                };
                if across_all_siblings {
                    self.resize_across_siblings(parent_id, branch, delta);
                } else {
                    let Some(index) = children.iter().position(|child| *child == branch) else {
                        return;
                    };
                    let neighbor = children.get(index + 1).copied().or_else(|| {
                        index
                            .checked_sub(1)
                            .and_then(|index| children.get(index).copied())
                    });
                    if let Some(neighbor) = neighbor {
                        self.resize_adjacent(branch, neighbor, delta);
                    }
                }
                return;
            }
            branch = parent_id;
            parent = *grandparent;
        }
    }

    fn resize_node_dimension_sway(&mut self, id: NodeId, width: bool, change: SizeChange) {
        let wanted = if width {
            Layout::SplitH
        } else {
            Layout::SplitV
        };
        let mut branch = id;
        let mut parent = self.nodes.get(&id).and_then(|node| node.parent);
        while let Some(parent_id) = parent {
            let Some(Node {
                parent: grandparent,
                value: TreeNode::Split {
                    layout, children, ..
                },
            }) = self.nodes.get(&parent_id)
            else {
                return;
            };
            if *layout == wanted && children.len() > 1 {
                let geometries = self.compute_geometry();
                let Some(parent_rect) = geometries.ipc_nodes.get(&parent_id) else {
                    return;
                };
                let parent_extent = if width {
                    parent_rect.size.w
                } else {
                    parent_rect.size.h
                };
                let child_extent = |child| {
                    geometries.ipc_nodes.get(child).map(|rect| {
                        if width {
                            rect.size.w
                        } else {
                            rect.size.h
                        }
                    })
                };
                let Some(current) = child_extent(&branch) else {
                    return;
                };
                let available = children
                    .iter()
                    .filter_map(child_extent)
                    .sum::<f64>()
                    .max(1.);
                let target = match change {
                    SizeChange::SetFixed(value) => f64::from(value),
                    SizeChange::SetProportion(value) => (parent_extent * value / 100.).trunc(),
                    SizeChange::AdjustFixed(_) | SizeChange::AdjustProportion(_) => return,
                };
                self.resize_across_siblings(parent_id, branch, (target - current) / available);
                return;
            }
            branch = parent_id;
            parent = *grandparent;
        }
    }

    fn resize_across_siblings(&mut self, parent: NodeId, target: NodeId, delta: f64) -> bool {
        if !delta.is_finite() {
            return false;
        }
        let old = self.compute_geometry();
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return false;
        };
        let Some(target_index) = children.iter().position(|child| *child == target) else {
            return false;
        };
        let compensation = delta / (children.len() - 1) as f64;
        if percents[target_index] + delta <= 0.
            || percents
                .iter()
                .enumerate()
                .any(|(index, percent)| index != target_index && percent - compensation <= 0.)
        {
            return false;
        }
        for (index, percent) in percents.iter_mut().enumerate() {
            *percent += if index == target_index {
                delta
            } else {
                -compensation
            };
        }
        self.request_window_sizes();
        self.animate_geometry_changes(old, None);
        true
    }

    fn resize_boundary(
        &self,
        id: NodeId,
        layout: Layout,
        toward_before: bool,
    ) -> Option<(NodeId, NodeId, f64, f64, f64, f64)> {
        let mut branch = id;
        let mut parent = self.nodes.get(&id)?.parent;
        while let Some(parent_id) = parent {
            let node = self.nodes.get(&parent_id)?;
            let TreeNode::Split {
                layout: parent_layout,
                children,
                percents,
            } = &node.value
            else {
                return None;
            };
            if Self::layouts_parallel(*parent_layout, layout) {
                let index = children.iter().position(|child| *child == branch)?;
                let neighbor_index = if toward_before {
                    index.checked_sub(1)
                } else {
                    Some(index + 1).filter(|index| *index < children.len())
                };
                if let Some(neighbor_index) = neighbor_index {
                    let neighbor = children[neighbor_index];
                    let axis_size = self.node_geometry(parent_id).map(|rect| {
                        let extent = if layout == Layout::SplitH {
                            rect.size.w
                        } else {
                            rect.size.h
                        };
                        extent - self.gaps * children.len().saturating_sub(1) as f64
                    })?;
                    let first = branch;
                    let second = neighbor;
                    let first_index = children.iter().position(|child| *child == first)?;
                    let second_index = children.iter().position(|child| *child == second)?;
                    let drag_sign = if toward_before { -1. } else { 1. };
                    return Some((
                        first,
                        second,
                        percents[first_index],
                        percents[second_index],
                        axis_size,
                        drag_sign,
                    ));
                }
            }
            branch = parent_id;
            parent = node.parent;
        }
        None
    }
}

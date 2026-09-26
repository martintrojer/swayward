use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn resize_adjacent(&mut self, first: NodeId, second: NodeId, delta: f64) -> bool {
        let old = self.compute_geometry();
        let changed = self.resize_adjacent_inner(first, second, delta);
        if changed {
            self.interactive_resize = None;
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

    pub fn resize_node_dimension_command(&mut self, id: NodeId, width: bool, change: SizeChange) {
        self.resize_node_dimension(id, width, change);
    }

    pub fn resize_node_edge_command(
        &mut self,
        id: NodeId,
        edge: ResizeEdge,
        change: SizeChange,
    ) -> bool {
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
            SizeChange::AdjustProportion(value) => {
                let parent_extent = self
                    .nodes
                    .get(&first)
                    .and_then(|node| node.parent)
                    .and_then(|parent| self.node_geometry(parent))
                    .map(|rect| if horizontal { rect.size.w } else { rect.size.h })
                    .unwrap_or(axis_size);
                parent_extent * value / 100. / axis_size.max(1.)
            }
            SizeChange::SetFixed(_) | SizeChange::SetProportion(_) => return false,
        };
        self.resize_adjacent(first, second, delta)
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
        self.resize_node_edge_command(id, edge, change)
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
            self.interactive_resize = None;
            self.request_window_sizes();
        }
    }

    pub fn toggle_window_width(&mut self, window: Option<&W::Id>, forwards: bool) {
        self.toggle_preset(window, true, forwards);
    }

    pub fn toggle_window_height(&mut self, window: Option<&W::Id>, forwards: bool) {
        self.toggle_preset(window, false, forwards);
    }

    pub fn expand_focused_to_available_width(&mut self) {
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
        // A corner resizes both axes, each against its own sibling boundary,
        // as sway's seatop_begin_resize_tiling does
        // (`sway/sway/input/seatop_resize_tiling.c:106-127`). An axis with no
        // boundary in that direction is skipped, not fatal.
        let axes: Vec<_> = [
            (true, edges.intersection(ResizeEdge::LEFT_RIGHT)),
            (false, edges.intersection(ResizeEdge::TOP_BOTTOM)),
        ]
        .into_iter()
        .filter(|(_, edge)| !edge.is_empty())
        .filter_map(|(horizontal, edge)| {
            let layout = if horizontal {
                Layout::SplitH
            } else {
                Layout::SplitV
            };
            let toward_before = edge.intersects(ResizeEdge::LEFT | ResizeEdge::TOP);
            let (first, second, initial_first, initial_second, axis_size, sign) =
                self.resize_boundary(id, layout, toward_before)?;
            Some(ResizeAxis {
                horizontal,
                first,
                second,
                initial_first,
                initial_second,
                axis_size,
                sign,
            })
        })
        .collect();
        if axes.is_empty() {
            return false;
        }
        self.interactive_resize = Some(InteractiveResize {
            window,
            target: id,
            axes,
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
        let axes = resize.axes.clone();
        let old = self.compute_geometry();
        let mut changed = false;
        for axis in axes {
            let moved = if axis.horizontal { delta.x } else { delta.y };
            let amount = moved * axis.sign / axis.axis_size.max(1.);
            let Some((current_first, current_second)) =
                self.sibling_percents(axis.first, axis.second)
            else {
                continue;
            };
            let target_first = axis.initial_first + amount;
            let target_second = axis.initial_second - amount;
            // Each axis stops at its own limit without holding up the other.
            if target_first <= 0. || target_second <= 0. {
                continue;
            }
            let change = target_first - current_first;
            if self.resize_adjacent_inner(axis.first, axis.second, change) {
                debug_assert!((current_second - change - target_second).abs() <= 1e-6);
                changed = true;
            }
        }
        if changed {
            self.animate_geometry_changes(old, None);
        }
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
                let extent = |rect: &Rectangle<f64, Logical>| {
                    if width {
                        rect.size.w
                    } else {
                        rect.size.h
                    }
                };
                let Some(parent_extent) = geometries.ipc_nodes.get(&parent_id).map(extent) else {
                    return;
                };
                let child_extent = |child| geometries.ipc_nodes.get(child).map(extent);
                let Some(current) = child_extent(&branch) else {
                    return;
                };
                let available = children
                    .iter()
                    .filter_map(child_extent)
                    .sum::<f64>()
                    .max(1.);
                let delta = match change {
                    SizeChange::AdjustFixed(value) => f64::from(value) / available,
                    SizeChange::AdjustProportion(value) => parent_extent * value / 100. / available,
                    SizeChange::SetFixed(value) => (f64::from(value) - current) / available,
                    SizeChange::SetProportion(value) => {
                        ((parent_extent * value / 100.).trunc() - current) / available
                    }
                };
                self.resize_across_siblings(parent_id, branch, delta);
                return;
            }
            branch = parent_id;
            parent = *grandparent;
        }
    }

    fn resize_node_dimension_sway(&mut self, id: NodeId, width: bool, change: SizeChange) {
        self.resize_node_dimension(id, width, change);
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
        self.interactive_resize = None;
        self.request_window_sizes();
        self.animate_geometry_changes(old, None);
        true
    }

    /// Whether `edge` of `window` borders a sibling rather than the
    /// workspace, following sway's `edge_is_external`
    /// (`sway/sway/input/seatop_default.c:39-74`): some ancestor with exactly
    /// the parallel split layout has a sibling on that side. A combined edge
    /// matches no layout in sway, so corners are always external.
    pub fn is_internal_edge(&self, window: &W::Id, edge: ResizeEdge) -> bool {
        let (wanted, before) = if edge == ResizeEdge::LEFT {
            (Layout::SplitH, true)
        } else if edge == ResizeEdge::RIGHT {
            (Layout::SplitH, false)
        } else if edge == ResizeEdge::TOP {
            (Layout::SplitV, true)
        } else if edge == ResizeEdge::BOTTOM {
            (Layout::SplitV, false)
        } else {
            return false;
        };
        let Some(mut id) = self.node_for_window(window) else {
            return false;
        };
        while let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) {
            if let Some(Node {
                value: TreeNode::Split {
                    layout, children, ..
                },
                ..
            }) = self.nodes.get(&parent)
            {
                if *layout == wanted {
                    if let Some(index) = children.iter().position(|child| *child == id) {
                        if (before && index > 0) || (!before && index + 1 < children.len()) {
                            return true;
                        }
                    }
                }
            }
            id = parent;
        }
        false
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

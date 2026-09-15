use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn active_window_visual_rectangle(&self) -> Option<Rectangle<f64, Logical>> {
        let id = self.focus?;
        let tile = self.tile(id)?;
        let mut rect = self.geometry(id)?;
        rect.loc += tile.window_loc();
        rect.size = tile.window_size();
        Rectangle::from_size(self.view_size).intersection(rect)
    }

    pub fn popup_target_rect(&self, window: &W::Id) -> Option<Rectangle<f64, Logical>> {
        let id = self.node_for_window(window)?;
        let tile = self.tile(id)?;
        let tile_rect = self.geometry(id)?;
        let mut target =
            Rectangle::from_size(Size::from((tile.window_size().w, self.parent_area.size.h)));
        target.loc.x = tile_rect.loc.x + tile.window_loc().x;
        target.loc.y = self.parent_area.loc.y;
        Some(target)
    }

    pub fn scroll_amount_to_activate(&self, _window: &W::Id) -> f64 {
        0.
    }

    pub fn render_above_top_layer(&self) -> bool {
        self.is_active_pending_fullscreen()
    }

    pub fn start_open_animation(&mut self, window: &W::Id) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        let Some(tile) = self.tile_mut(id) else {
            return false;
        };
        tile.start_open_animation();
        true
    }

    pub fn update_render_elements(&mut self, is_active: bool, layer: RenderLayer) {
        let focus = self.focus;
        let geometries = self.compute_geometry();
        let visible = self.visible_leaves();
        for (id, node) in &mut self.nodes {
            let TreeNode::Leaf { tile } = &mut node.value else {
                continue;
            };
            if layer.is_normal() == tile.is_moving_between_workspaces()
                || (!visible.contains(id) && tile.alpha_animation.is_none())
            {
                continue;
            }
            let Some(rect) = geometries.leaf_contents.get(id) else {
                continue;
            };
            let mut view_rect = Rectangle::from_size(self.view_size);
            view_rect.loc -= rect.loc + tile.render_offset();
            tile.update_render_elements(is_active && Some(*id) == focus, view_rect);
        }
        self.update_tab_indicators(is_active, &geometries.leaf_contents);
    }

    pub fn tiles_with_render_positions(
        &self,
    ) -> impl Iterator<Item = (&Tile<W>, Point<f64, Logical>, bool)> {
        let geometries = self.compute_geometry();
        let visible = self.visible_leaves();
        let scale = self.scale;
        self.iter_depth_first().filter_map(move |(id, node)| {
            let TreeNode::Leaf { tile } = node else {
                return None;
            };
            let rect = geometries.leaf_contents.get(&id)?;
            let pos = (rect.loc + tile.render_offset())
                .to_physical_precise_round(scale)
                .to_logical(scale);
            Some((tile.as_ref(), pos, visible.contains(&id)))
        })
    }

    pub fn tiles_with_render_positions_mut(
        &mut self,
        round: bool,
    ) -> impl Iterator<Item = (&mut Tile<W>, Point<f64, Logical>)> {
        let geometries = self.compute_geometry();
        let scale = self.scale;
        self.nodes.iter_mut().filter_map(move |(id, node)| {
            let TreeNode::Leaf { tile } = &mut node.value else {
                return None;
            };
            let mut pos = geometries.leaf_contents.get(id)?.loc + tile.render_offset();
            if round {
                pos = pos.to_physical_precise_round(scale).to_logical(scale);
            }
            Some((tile.as_mut(), pos))
        })
    }

    pub fn tiles_with_ipc_layouts(&self) -> impl Iterator<Item = (&Tile<W>, WindowLayout)> {
        let geometries = self.compute_geometry();
        self.iter_depth_first().filter_map(move |(id, node)| {
            let TreeNode::Leaf { tile } = node else {
                return None;
            };
            let mut layout = tile.ipc_layout_template();
            layout.tile_pos_in_workspace_view = geometries
                .leaf_contents
                .get(&id)
                .map(|rect| rect.loc.into());
            Some((tile.as_ref(), layout))
        })
    }

    pub fn tab_indicator_focus_target(&self, window: &W::Id) -> Option<&W> {
        let id = self.node_for_window(window)?;
        let candidates = self.nodes.iter().filter_map(|(parent, node)| {
            let TreeNode::Split {
                layout: Layout::Tabbed | Layout::Stacked,
                children,
                ..
            } = &node.value
            else {
                return None;
            };
            children
                .iter()
                .find(|child| self.first_leaf_in(**child) == Some(id))
                .map(|branch| (*parent, *branch))
        });
        let (_, represented) = candidates.max_by_key(|(parent, _)| {
            let mut depth = 0;
            let mut node = *parent;
            while let Some(next) = self.nodes.get(&node).and_then(|node| node.parent) {
                depth += 1;
                node = next;
            }
            depth
        })?;
        let parent = self.nodes.get(&represented)?.parent?;
        let focused = self.focused_leaf_in(parent)?;
        self.tile(focused).map(|tile| tile.window())
    }

    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<(&W, HitType)> {
        let geometries = self.compute_geometry();
        let titlebar = geometries
            .titlebars
            .iter()
            .filter(|(_, titlebar)| titlebar.rect.contains(pos))
            .max_by_key(|(id, _)| self.node_depth(**id))
            .map(|(_, titlebar)| titlebar);
        if let Some(titlebar) = titlebar {
            let id = self.node_for_window(&titlebar.target)?;
            let tile = self.tile(id)?;
            return Some((
                tile.window(),
                HitType::Activate {
                    is_tab_indicator: true,
                },
            ));
        }
        for (split, indicator) in &self.tab_indicators {
            let Some((area, children)) = self.tab_area(*split, &geometries.leaf_contents) else {
                continue;
            };
            if let Some(index) = indicator.hit(area, children.len(), self.scale, pos) {
                if let Some(tile) = children.get(index).and_then(|id| self.first_tile_in(*id)) {
                    return Some((
                        tile.window(),
                        HitType::Activate {
                            is_tab_indicator: true,
                        },
                    ));
                }
            }
        }
        self.tiles_with_render_positions()
            .filter(|(_, _, visible)| *visible)
            .find_map(|(tile, tile_pos, _)| HitType::hit_tile(tile, tile_pos, pos))
    }

    pub fn start_close_animation_for_window(
        &mut self,
        renderer: &mut GlesRenderer,
        window: &W::Id,
        blocker: TransactionBlocker,
    ) {
        let Some(id) = self.node_for_window(window) else {
            return;
        };
        let Some(pos) = self
            .geometry(id)
            .and_then(|rect| self.tile(id).map(|tile| rect.loc + tile.render_offset()))
        else {
            return;
        };
        let Some(tile) = self.tile_mut(id) else {
            return;
        };
        let Some(snapshot) = tile.take_unmap_snapshot() else {
            return;
        };
        let size = tile.tile_size();
        let anim = crate::animation::Animation::new(
            self.clock.clone(),
            0.,
            1.,
            0.,
            self.options.animations.window_close.anim,
        );
        let blocker = if self.options.disable_transactions {
            TransactionBlocker::completed()
        } else {
            blocker
        };
        match ClosingWindow::new(
            renderer,
            snapshot,
            Scale::from(self.scale),
            size,
            pos,
            blocker,
            anim,
        ) {
            Ok(closing) => self.closing_windows.push(closing),
            Err(err) => warn!("error creating a closing window animation: {err:?}"),
        }
    }

    pub fn render<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        xray_pos: XrayPos,
        focus_ring: bool,
        layer: RenderLayer,
        push: &mut dyn FnMut(TilingTreeRenderElement<R>),
    ) {
        let scale = Scale::from(self.scale);
        if layer.is_normal() {
            let view = Rectangle::from_size(self.view_size);
            for closing in self.closing_windows.iter().rev() {
                push(closing.render(ctx.as_gles(), view, scale).into());
            }
        }
        let focus = self.focus;
        for indicator in self.tab_indicators.values() {
            indicator.render(ctx.renderer, Point::default(), &mut |element| {
                push(element.into())
            });
        }
        let geometries = self.compute_geometry();
        let visible = self.visible_leaves();
        self.titlebars.retain(geometries.titlebars.keys().copied());
        for (id, titlebar) in &geometries.titlebars {
            let mut titlebar = titlebar.clone();
            titlebar.state = self.titlebar_state(*id, focus_ring);
            if titlebar.visible {
                if let Some(element) = self.titlebars.render(
                    ctx.renderer,
                    *id,
                    &titlebar,
                    self.scale,
                    &self.options.layout.titlebar,
                ) {
                    push(element.into());
                }
            }
        }
        for (id, node) in self.iter_depth_first() {
            let TreeNode::Leaf { tile } = node else {
                continue;
            };
            if (!visible.contains(&id) && tile.alpha_animation.is_none())
                || layer.is_normal() == tile.is_moving_between_workspaces()
            {
                continue;
            }
            let Some(rect) = geometries.leaf_contents.get(&id) else {
                continue;
            };
            let tile_pos = (rect.loc + tile.render_offset())
                .to_physical_precise_round(self.scale)
                .to_logical(self.scale);
            let xray = xray_pos.offset(tile_pos);
            tile.render(
                ctx.r(),
                tile_pos,
                xray,
                focus_ring && Some(id) == focus,
                &mut |element| push(element.into()),
            );
        }
    }

    pub fn update_window(&mut self, window: &W::Id, serial: Option<Serial>) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        let Some(tile) = self.tile_mut(id) else {
            return false;
        };
        if let Some(serial) = serial {
            tile.window_mut().on_commit(serial);
        }
        tile.update_window();
        true
    }

    pub fn refresh(&mut self, is_active: bool, is_focused: bool) {
        let focus = self.focus;
        let resize = self
            .interactive_resize
            .as_ref()
            .map(|resize| (resize.window.clone(), resize.data));
        let individual =
            self.options.disable_transactions || self.options.disable_resize_throttling;
        let shared_intent = if individual {
            ConfigureIntent::CanSend
        } else {
            self.tiles()
                .fold(ConfigureIntent::NotNeeded, |intent, tile| {
                    match (intent, tile.window().configure_intent()) {
                        (_, ConfigureIntent::ShouldSend) => ConfigureIntent::ShouldSend,
                        (ConfigureIntent::NotNeeded, next) => next,
                        (ConfigureIntent::CanSend, ConfigureIntent::Throttled) => {
                            ConfigureIntent::Throttled
                        }
                        (intent, _) => intent,
                    }
                })
        };
        for (id, node) in &mut self.nodes {
            let TreeNode::Leaf { tile } = &mut node.value else {
                continue;
            };
            let window = tile.window_mut();
            let focused = Some(*id) == focus;
            window.set_active_in_column(focused);
            window.set_floating(false);
            window.set_activated(
                is_active && (!self.options.deactivate_unfocused_windows || focused && is_focused),
            );
            window.set_interactive_resize(
                resize
                    .as_ref()
                    .and_then(|(target, data)| (window.id() == target).then_some(*data)),
            );
            let border = self
                .options
                .layout
                .border
                .merged_with(&window.rules().border);
            let padding = self.gaps * 2. + if border.off { 0. } else { border.width * 2. };
            let bounds = Size::from((
                (self.parent_area.size.w - padding).max(1.),
                (self.parent_area.size.h - padding).max(1.),
            ));
            window.set_bounds(bounds.to_i32_floor());
            let intent = if individual {
                window.configure_intent()
            } else {
                shared_intent
            };
            if matches!(
                intent,
                ConfigureIntent::CanSend | ConfigureIntent::ShouldSend
            ) {
                window.send_pending_configure();
            }
            window.refresh();
        }
    }

    pub fn view_offset_gesture_begin(&mut self, _is_touchpad: bool) {}

    pub fn view_offset_gesture_update(
        &mut self,
        _delta_x: f64,
        _timestamp: Duration,
        _is_touchpad: bool,
    ) -> Option<bool> {
        None
    }

    pub fn view_offset_gesture_end(&mut self, _is_touchpad: Option<bool>) -> bool {
        false
    }

    pub fn dnd_scroll_gesture_begin(&mut self) {}

    pub fn dnd_scroll_gesture_scroll(&mut self, _delta: f64) -> bool {
        false
    }

    pub fn dnd_scroll_gesture_end(&mut self) {}

    pub fn has_view_offset_gesture(&self) -> bool {
        false
    }

    pub fn view_pos(&self) -> f64 {
        0.
    }

    pub fn active_column_idx(&self) -> usize {
        self.focus
            .and_then(|id| self.root_branch(id))
            .and_then(|branch| self.root_children()?.iter().position(|id| *id == branch))
            .unwrap_or(0)
    }

    fn node_depth(&self, mut id: NodeId) -> usize {
        let mut depth = 0;
        while let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) {
            depth += 1;
            id = parent;
        }
        depth
    }

    fn first_tile_in(&self, id: NodeId) -> Option<&Tile<W>> {
        self.first_leaf_in(id).and_then(|id| self.tile(id))
    }

    fn tab_area(
        &self,
        id: NodeId,
        geometries: &HashMap<NodeId, Rectangle<f64, Logical>>,
    ) -> Option<(Rectangle<f64, Logical>, Vec<NodeId>)> {
        let TreeNode::Split {
            layout, children, ..
        } = &self.nodes.get(&id)?.value
        else {
            return None;
        };
        if *layout != Layout::Tabbed || children.is_empty() {
            return None;
        }
        let mut area = None;
        for child in children {
            let leaf = self.first_leaf_in(*child)?;
            let rect = *geometries.get(&leaf)?;
            area = Some(area.map_or(rect, |mut area: Rectangle<f64, Logical>| {
                let right = (area.loc.x + area.size.w).max(rect.loc.x + rect.size.w);
                let bottom = (area.loc.y + area.size.h).max(rect.loc.y + rect.size.h);
                area.loc.x = area.loc.x.min(rect.loc.x);
                area.loc.y = area.loc.y.min(rect.loc.y);
                area.size.w = right - area.loc.x;
                area.size.h = bottom - area.loc.y;
                area
            }));
        }
        Some((area?, children.clone()))
    }

    fn update_tab_indicators(
        &mut self,
        is_active: bool,
        geometries: &HashMap<NodeId, Rectangle<f64, Logical>>,
    ) {
        let tabbed: Vec<_> = self
            .nodes
            .iter()
            .filter_map(|(id, node)| match &node.value {
                TreeNode::Split {
                    layout: Layout::Tabbed,
                    children,
                    ..
                } => Some((*id, children.clone())),
                _ => None,
            })
            .collect();
        self.tab_indicators
            .retain(|id, _| tabbed.iter().any(|(tabbed, _)| tabbed == id));
        self.tab_active
            .retain(|id, _| tabbed.iter().any(|(tabbed, _)| tabbed == id));
        for (id, children) in tabbed {
            let Some((area, _)) = self.tab_area(id, geometries) else {
                continue;
            };
            let active = self
                .focus
                .and_then(|focus| {
                    children
                        .iter()
                        .find(|child| self.contains_node(**child, focus))
                })
                .copied()
                .or_else(|| children.first().copied());
            if self.tab_active.get(&id).copied() != active {
                let movement = self.options.animations.window_movement.0;
                let previous = self.tab_active.insert(id, active.unwrap_or(id));
                for child in &children {
                    let Some(leaf) = self.first_leaf_in(*child) else {
                        continue;
                    };
                    if let Some(tile) = self.tile_mut(leaf) {
                        if Some(*child) == active {
                            tile.ensure_alpha_animates_to_1();
                        } else if previous.is_none() || previous == Some(*child) {
                            tile.animate_alpha(1., 0., movement);
                        }
                    }
                }
            }
            let config = self.options.layout.tab_indicator;
            let tabs: Vec<_> = children
                .iter()
                .filter_map(|child| {
                    let leaf = self.first_leaf_in(*child)?;
                    let tile = self.tile(leaf)?;
                    let rect = geometries.get(&leaf)?;
                    Some(TabInfo::from_tile(
                        tile,
                        rect.loc,
                        self.focus
                            .is_some_and(|focus| self.contains_node(*child, focus)),
                        tile.window().is_urgent(),
                        &config,
                    ))
                })
                .collect();
            let is_new = !self.tab_indicators.contains_key(&id);
            let indicator = self
                .tab_indicators
                .entry(id)
                .or_insert_with(|| TabIndicator::new(config));
            if is_new {
                indicator.start_open_animation(
                    self.clock.clone(),
                    self.options.animations.window_open.anim,
                );
            }
            indicator.update_render_elements(
                true,
                area,
                Rectangle::from_size(self.view_size),
                tabs.len(),
                tabs.into_iter(),
                is_active,
                self.scale,
            );
        }
    }
}

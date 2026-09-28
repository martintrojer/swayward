use super::*;

impl<W: LayoutElement> FloatingLayout<W> {
    pub fn interactive_resize_begin(&mut self, window: W::Id, edges: ResizeEdge) -> bool {
        if self.interactive_resize.is_some() {
            return false;
        }

        let Some(tile) = self
            .entries
            .iter_mut()
            .map(|entry| &mut entry.tile)
            .find(|tile| tile.window().id() == &window)
        else {
            return false;
        };

        let original_window_size = tile.window_size();

        let resize = InteractiveResize {
            window,
            original_window_size,
            data: InteractiveResizeData { edges },
        };
        self.interactive_resize = Some(resize);

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

        if window != &resize.window {
            return false;
        }

        let original_window_size = resize.original_window_size;
        let edges = resize.data.edges;

        if edges.intersects(ResizeEdge::LEFT_RIGHT) {
            let mut dx = delta.x;
            if edges.contains(ResizeEdge::LEFT) {
                dx = -dx;
            };

            let window_width = (original_window_size.w + dx).round() as i32;
            self.set_window_width(
                Some(window),
                SizeChange::SetFixed(window_width),
                false,
                self.view_size.to_i32_round(),
            );
        }

        if edges.intersects(ResizeEdge::TOP_BOTTOM) {
            let mut dy = delta.y;
            if edges.contains(ResizeEdge::TOP) {
                dy = -dy;
            };

            let window_height = (original_window_size.h + dy).round() as i32;
            self.set_window_height(
                Some(window),
                SizeChange::SetFixed(window_height),
                false,
                self.view_size.to_i32_round(),
            );
        }

        true
    }

    pub fn interactive_resize_end(&mut self, window: Option<&W::Id>) {
        let Some(resize) = &self.interactive_resize else {
            return;
        };

        if let Some(window) = window {
            if window != &resize.window {
                return;
            }
        }

        self.interactive_resize = None;
    }

    pub fn refresh(&mut self, is_active: bool, is_focused: bool) {
        for entry in &mut self.tree_entries {
            entry.tree.refresh_floating(is_active, is_focused);
        }
        let active = self.active_window_id.clone();
        for entry in &mut self.entries {
            let win = entry.tile.window_mut();

            win.set_active_in_column(true);
            win.set_floating(true);

            let mut is_active = is_active && Some(win.id()) == active.as_ref();
            if self.options.deactivate_unfocused_windows {
                is_active &= is_focused;
            }
            win.set_activated(is_active);

            let resize_data = self
                .interactive_resize
                .as_ref()
                .filter(|resize| &resize.window == win.id())
                .map(|resize| resize.data);
            win.set_interactive_resize(resize_data);

            let border_config = self.options.layout.border.merged_with(&win.rules().border);
            let bounds = compute_toplevel_bounds(border_config, self.working_area.size);
            win.set_bounds(bounds);

            // If transactions are disabled, also disable combined throttling, for more
            // intuitive behavior.
            let intent = if self.options.disable_resize_throttling {
                ConfigureIntent::CanSend
            } else {
                win.configure_intent()
            };

            if matches!(
                intent,
                ConfigureIntent::CanSend | ConfigureIntent::ShouldSend
            ) {
                win.send_pending_configure();
            }

            win.refresh();
        }
    }

    pub fn clamp_within_working_area(
        &self,
        pos: Point<f64, Logical>,
        size: Size<f64, Logical>,
    ) -> Point<f64, Logical> {
        let mut rect = Rectangle::new(pos, size);
        clamp_preferring_top_left_in_area(self.working_area, &mut rect);
        rect.loc
    }

    pub fn scale_by_working_area(&self, pos: Point<f64, SizeFrac>) -> Point<f64, Logical> {
        Data::scale_by_working_area(self.working_area, pos)
    }

    pub fn logical_to_size_frac(&self, logical_pos: Point<f64, Logical>) -> Point<f64, SizeFrac> {
        Data::logical_to_size_frac_in_working_area(self.working_area, logical_pos)
    }

    pub(super) fn move_and_animate(&mut self, idx: usize, new_pos: Point<f64, Logical>) {
        // Moves up to this logical pixel distance are not animated.
        const ANIMATION_THRESHOLD_SQ: f64 = 10. * 10.;

        let entry = &mut self.entries[idx];
        let tile = &mut entry.tile;
        let data = &mut entry.data;

        let prev_pos = data.logical_pos;
        data.set_logical_pos(new_pos);
        let new_pos = data.logical_pos;

        let diff = prev_pos - new_pos;
        if diff.x * diff.x + diff.y * diff.y > ANIMATION_THRESHOLD_SQ {
            tile.animate_move_from(prev_pos - new_pos);
        }
    }

    pub fn new_window_size(
        &self,
        width: Option<PresetSize>,
        height: Option<PresetSize>,
        rules: &ResolvedWindowRules,
    ) -> Size<i32, Logical> {
        let border = self.options.layout.border.merged_with(&rules.border);

        let resolve = |size: Option<PresetSize>, working_area_size: f64| {
            if let Some(size) = size {
                let size = match resolve_preset_size(size, working_area_size) {
                    ResolvedSize::Tile(mut size) => {
                        if !border.off {
                            size -= border.width * 2.;
                        }
                        size
                    }
                    ResolvedSize::Window(size) => size,
                };

                max(1, size.floor() as i32)
            } else {
                0
            }
        };

        let width = resolve(width, self.working_area.size.w);
        let height = resolve(height, self.working_area.size.h);

        Size::from((width, height))
    }

    pub fn remap_stored_tile_pos(
        &self,
        tile: &mut Tile<W>,
        old_area: Option<Rectangle<f64, Logical>>,
    ) {
        let Some(old_area) = old_area.filter(|area| area.size.w > 0. && area.size.h > 0.) else {
            tile.floating_pos = None;
            return;
        };
        let Some(pos) = tile.floating_pos else {
            return;
        };
        let old_pos = Data::scale_by_working_area(old_area, pos);
        let size = tile.tile_size();
        let old_center = old_pos + size.downscale(2.);
        let relative_center = old_center - old_area.loc;
        let new_center = Point::from((
            self.working_area.loc.x
                + relative_center.x * self.working_area.size.w / old_area.size.w,
            self.working_area.loc.y
                + relative_center.y * self.working_area.size.h / old_area.size.h,
        ));
        tile.floating_pos = Some(self.logical_to_size_frac(new_center - size.downscale(2.)));
    }

    pub fn stored_or_default_tile_pos(&self, tile: &Tile<W>) -> Option<Point<f64, Logical>> {
        let pos = tile.floating_pos.map(|pos| self.scale_by_working_area(pos));
        pos.or_else(|| {
            tile.window().rules().default_floating_position.map(|pos| {
                let relative_to = pos.relative_to;
                let size = tile.tile_size();
                let area = self.working_area;

                let mut pos = Point::from((pos.x.0, pos.y.0));
                if relative_to == RelativeTo::TopRight
                    || relative_to == RelativeTo::BottomRight
                    || relative_to == RelativeTo::Right
                {
                    pos.x = area.size.w - size.w - pos.x;
                }
                if relative_to == RelativeTo::BottomLeft
                    || relative_to == RelativeTo::BottomRight
                    || relative_to == RelativeTo::Bottom
                {
                    pos.y = area.size.h - size.h - pos.y;
                }
                if relative_to == RelativeTo::Top || relative_to == RelativeTo::Bottom {
                    pos.x += area.size.w / 2.0 - size.w / 2.0
                }
                if relative_to == RelativeTo::Left || relative_to == RelativeTo::Right {
                    pos.y += area.size.h / 2.0 - size.h / 2.0
                }

                pos + self.working_area.loc
            })
        })
    }

    #[cfg(test)]
    pub fn view_size(&self) -> Size<f64, Logical> {
        self.view_size
    }

    pub fn working_area(&self) -> Rectangle<f64, Logical> {
        self.working_area
    }

    #[cfg(test)]
    pub fn scale(&self) -> f64 {
        self.scale
    }

    #[cfg(test)]
    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    #[cfg(test)]
    pub fn options(&self) -> &Rc<Options> {
        &self.options
    }

    #[cfg(test)]
    pub fn verify_invariants(&self) {
        assert!(self.scale > 0.);
        assert!(self.scale.is_finite());
        for entry in &self.tree_entries {
            assert_eq!(entry.tree.parent_area(), entry.rect);
            assert!(
                !entry.tree.is_empty(),
                "floating tree entry must not be empty"
            );
            assert!(entry.tree.contains(entry.root));
            entry.tree.verify_invariants();
        }
        let mut node_ids = std::collections::HashSet::new();
        for entry in &self.tree_entries {
            for (id, _) in entry.tree.iter_depth_first() {
                assert!(
                    node_ids.insert(id),
                    "a node must belong to exactly one floating tree"
                );
            }
        }

        for (i, (tile, data)) in self
            .entries
            .iter()
            .map(|entry| (&entry.tile, &entry.data))
            .enumerate()
        {
            use crate::layout::SizingMode;

            assert!(Rc::ptr_eq(&self.options, &tile.options));
            assert_eq!(self.view_size, tile.view_size());
            assert_eq!(self.clock, tile.clock);
            assert_eq!(self.scale, tile.scale());
            tile.verify_invariants();

            if let Some(idx) = tile.floating_preset_width_idx {
                assert!(idx < self.options.layout.preset_column_widths.len());
            }
            if let Some(idx) = tile.floating_preset_height_idx {
                assert!(idx < self.options.layout.preset_window_heights.len());
            }

            assert_eq!(
                tile.window().pending_sizing_mode(),
                SizingMode::Normal,
                "floating windows cannot be maximized or fullscreen"
            );

            data.verify_invariants();

            let mut data2 = *data;
            data2.update(tile);
            data2.update_config(self.view_size, self.working_area);
            assert_eq!(data, &data2, "tile data must be up to date");

            for entry_below in &self.entries[i + 1..] {
                assert_ne!(
                    entry_below.tile.window().id(),
                    tile.window().id(),
                    "a window must belong to exactly one floating entry"
                );
                assert!(
                    !entry_below.tile.window().is_child_of(tile.window()),
                    "children must be stacked above parents"
                );
            }
        }

        if let Some(id) = &self.active_window_id {
            assert!(!self.is_empty());
            assert!(self.contains(id), "active window must be present in tiles");
        } else {
            assert!(self.is_empty());
        }

        if let Some(resize) = &self.interactive_resize {
            assert!(
                self.contains(&resize.window),
                "interactive resize window must be present in tiles"
            );
        }
    }
}

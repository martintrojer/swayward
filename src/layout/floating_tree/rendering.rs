use super::*;

impl<W: LayoutElement> FloatingLayout<W> {
    pub fn render<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        xray_pos: XrayPos,
        view_rect: Rectangle<f64, Logical>,
        focus_ring: bool,
        layer: RenderLayer,
        push: &mut dyn FnMut(FloatingLayoutRenderElement<R>),
    ) {
        let scale = Scale::from(self.scale);

        let active = self.active_window_id.clone();
        let workspace_focused = focus_ring;
        let tiles: Vec<_> = self.tiles_with_render_positions().collect();
        let closing_indices: Vec<_> = self
            .closing_windows
            .iter()
            .map(|(index, _)| *index)
            .collect();
        // Closing snapshots sit in their former slot among the single windows
        // (see `closing_windows`); each is drawn just before (above) the live
        // single window that now holds its slot, or after them all.
        let mut closing_above: Vec<Vec<usize>> = vec![Vec::new(); tiles.len() + 1];
        let mut live = 0;
        for element in floating_stack_order(tiles.len(), &closing_indices) {
            match element {
                FloatingStackElement::Closing(closing) => closing_above[live].push(closing),
                FloatingStackElement::Live(_) => live += 1,
            }
        }
        // Elements are pushed front to back, so walk the shared stack top
        // first.
        for slot in self.stacking() {
            let window = match slot {
                StackSlot::Tree(root) => {
                    if let Some(entry) = self.tree_entries.iter().find(|entry| entry.root == root) {
                        entry
                            .tree
                            .render(ctx.r(), xray_pos, focus_ring, layer, &mut |element| {
                                push(element.into())
                            });
                    }
                    continue;
                }
                StackSlot::Window(window) => window,
            };
            let Some(index) = self
                .entries
                .iter()
                .position(|entry| entry.tile.window().id() == &window)
            else {
                continue;
            };
            if layer.is_normal() {
                for &closing in &closing_above[index] {
                    let (_, closing) = &self.closing_windows[closing];
                    push(closing.render(ctx.as_gles(), view_rect, scale).into());
                }
            }
            let (tile, tile_pos) = tiles[index];
            let entry = &self.entries[index];
            // Skip tiles belonging to a different render layer.
            if layer.is_normal() == tile.is_moving_between_workspaces() {
                continue;
            }

            // For the active tile, draw the focus ring.
            let focused = Some(tile.window().id()) == active.as_ref();
            let focus_ring = focus_ring && focused;

            if let Some(rect) = self.titlebar_rect(tile, tile_pos, tile.animated_tile_size().w) {
                let titlebar = Titlebar {
                    target: tile.window().id().clone(),
                    rect,
                    ipc_rect: Rectangle::default(),
                    title: tile.window().title(),
                    marks: tile.window().marks(),
                    state: if tile.window().is_urgent() {
                        TitlebarState::Urgent
                    } else if focused && workspace_focused {
                        TitlebarState::Focused
                    } else if focused {
                        TitlebarState::FocusedInactive
                    } else {
                        TitlebarState::Unfocused
                    },
                    visible: true,
                };
                let radius = tile.window().geometry_corner_radius();
                if let Some(element) = entry.titlebar.render(
                    ctx.renderer,
                    &titlebar,
                    self.scale,
                    &self.options.layout.titlebar,
                    (f64::from(radius.top_left), f64::from(radius.top_right)),
                ) {
                    push(element.into());
                }
            }

            let xray_pos = xray_pos.offset(tile_pos);
            tile.render(ctx.r(), tile_pos, xray_pos, focus_ring, &mut |elem| {
                push(elem.into())
            });
        }
        if layer.is_normal() {
            for &closing in &closing_above[tiles.len()] {
                let (_, closing) = &self.closing_windows[closing];
                push(closing.render(ctx.as_gles(), view_rect, scale).into());
            }
        }
    }
}

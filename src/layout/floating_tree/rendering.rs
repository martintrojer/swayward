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

        // Draw the closing windows on top of the other windows.
        //
        // Preserving the stacking order instead is tracked by mu task
        // layout-closing-window-stack-order.
        if layer.is_normal() {
            for closing in self.closing_windows.iter().rev() {
                let elem = closing.render(ctx.as_gles(), view_rect, scale);
                push(elem.into());
            }
        }

        for entry in self.tree_entries.iter().rev() {
            entry
                .tree
                .render(ctx.r(), xray_pos, focus_ring, layer, &mut |element| {
                    push(element.into())
                });
        }
        let active = self.active_window_id.clone();
        let workspace_focused = focus_ring;
        for (entry, (tile, tile_pos)) in self.entries.iter().zip(self.tiles_with_render_positions())
        {
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
    }
}

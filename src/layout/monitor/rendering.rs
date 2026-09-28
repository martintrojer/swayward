use super::*;

impl<W: LayoutElement> Monitor<W> {
    pub fn render_above_top_layer(&self) -> bool {
        // Render above the top layer only if the view is stationary.
        if self.workspace_switch.is_some() || self.overview_progress.is_some() {
            return false;
        }

        let ws = &self.workspaces[self.active_workspace_idx];
        ws.render_above_top_layer()
    }

    pub fn render_insert_hint_between_workspaces<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        push: &mut dyn FnMut(MonitorRenderElement<R>),
    ) {
        if self.options.layout.insert_hint.off {
            return;
        }
        let Some(render_loc) = self.insert_hint_render_loc else {
            return;
        };
        let InsertWorkspace::Preview(_) = render_loc.workspace else {
            return;
        };

        self.insert_hint_element
            .render(renderer, render_loc.location, &mut |elem| {
                let elem = MonitorInnerRenderElement::UncroppedInsertHint(elem);
                let elem = RescaleRenderElement::from_element(elem, Point::default(), 1.);
                let elem =
                    RelocateRenderElement::from_element(elem, Point::default(), Relocate::Relative);
                push(elem);
            });
    }

    pub fn render_workspaces<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        focus_ring: bool,
        push: &mut dyn FnMut(MonitorRenderElement<R>),
    ) {
        let _span = tracy_client::span!("Monitor::render_workspaces");

        let scale = self.scale.fractional_scale();
        // Ceil the height in physical pixels.
        let height = (self.view_size.h * scale).ceil() as i32;

        let zoom = self.overview_zoom();

        let insert_hint_render_loc = self
            .insert_hint_render_loc
            .filter(|_| !self.options.layout.insert_hint.off);

        let scale_relocate = move |geo: Rectangle<f64, Logical>, elem| {
            let elem = RescaleRenderElement::from_element(elem, Point::from((0, 0)), zoom);
            RelocateRenderElement::from_element(
                elem,
                // The offset we get from workspaces_with_render_geo() is already
                // rounded to physical pixels, but it's in the logical coordinate
                // space, so we need to convert it to physical.
                geo.loc.to_physical_precise_round(scale),
                Relocate::Relative,
            )
        };

        // Draw in passes for correct Z ordering during window movement between workspaces:
        // - floating windows moving between workspaces
        // - normal floating windows
        // - tiled windows moving between workspaces
        // - normal tiled windows
        for pass in 0..4 {
            // Don't cull when drawing windows moving between workspaces so that windows moving to
            // workspaces off-screen will still render.
            let cull = matches!(pass, 1 | 3);

            // Crop the elements to prevent them overflowing, currently visible during a workspace
            // switch.
            //
            // HACK: crop to infinite bounds at least horizontally where we
            // know there's no workspace joining or monitor bounds, otherwise
            // it will cut pixel shaders and mess up the coordinate space.
            // There's also a damage tracking bug which causes glitched
            // rendering for maximized GTK windows.
            //
            // Proper workspace bounds depend on the Crop coordinate and damage fixes tracked by
            // mu task layout-crop-bounds.
            //
            // Also, check cull here to avoid cropping windows moving between workspaces.
            //
            // Mu task layout-crop-bounds also tracks a workspace-height crop for moving windows,
            // which prevents overflow from appearing and disappearing.
            let crop_bounds =
                if cull && (self.workspace_switch.is_some() || self.overview_progress.is_some()) {
                    Rectangle::new(
                        Point::from((-i32::MAX / 2, 0)),
                        Size::from((i32::MAX, height)),
                    )
                } else {
                    Rectangle::new(
                        Point::from((-i32::MAX / 2, -i32::MAX / 2)),
                        Size::from((i32::MAX, i32::MAX)),
                    )
                };

            for (ws, geo) in self.workspaces_with_render_geo_cull(cull) {
                // Macro instead of closure because ws and insert hint have different elem types.
                macro_rules! push {
                    () => {{
                        &mut |elem| {
                            let elem = CropRenderElement::from_element(elem, scale, crop_bounds);
                            if let Some(elem) = elem {
                                let elem = MonitorInnerRenderElement::from(elem);
                                push(scale_relocate(geo, elem));
                            }
                        }
                    }};
                }

                let xray_pos = XrayPos::new(geo.loc, zoom);

                match pass {
                    0 => {
                        ws.render_floating(
                            ctx.r(),
                            xray_pos,
                            focus_ring,
                            RenderLayer::MovingBetweenWorkspaces,
                            push!(),
                        );
                    }
                    1 => {
                        ws.render_floating(
                            ctx.r(),
                            xray_pos,
                            focus_ring,
                            RenderLayer::Normal,
                            push!(),
                        );

                        if let Some(loc) = insert_hint_render_loc {
                            if loc.workspace == InsertWorkspace::Existing(ws.id()) {
                                self.insert_hint_element.render(
                                    ctx.renderer,
                                    loc.location,
                                    push!(),
                                );
                            }
                        }
                    }
                    2 => {
                        ws.render_scrolling(
                            ctx.r(),
                            xray_pos,
                            focus_ring,
                            RenderLayer::MovingBetweenWorkspaces,
                            push!(),
                        );
                    }
                    _ => {
                        ws.render_scrolling(
                            ctx.r(),
                            xray_pos,
                            focus_ring,
                            RenderLayer::Normal,
                            push!(),
                        );
                    }
                }
            }
        }
    }

    pub fn render_workspace_shadows<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        push: &mut dyn FnMut(MonitorRenderElement<R>),
    ) {
        let Some(progress) = self.overview_progress.as_ref().map(|p| p.clamped_value()) else {
            return;
        };
        let alpha = progress.clamp(0., 1.) as f32;

        let _span = tracy_client::span!("Monitor::render_workspace_shadows");

        let scale = self.scale.fractional_scale();
        let zoom = self.overview_zoom();

        for (ws, geo) in self.workspaces_with_render_geo() {
            ws.render_shadow(renderer, &mut |elem| {
                let elem = elem.with_alpha(alpha);
                let elem = MonitorInnerRenderElement::Shadow(elem);
                let elem = RescaleRenderElement::from_element(elem, Point::from((0, 0)), zoom);
                let elem = RelocateRenderElement::from_element(
                    elem,
                    geo.loc.to_physical_precise_round(scale),
                    Relocate::Relative,
                );
                push(elem);
            });
        }
    }
}

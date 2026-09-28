use super::*;

impl<W: LayoutElement> Workspace<W> {
    pub fn render_scrolling<R: NiriRenderer>(
        &self,
        ctx: RenderCtx<R>,
        xray_pos: XrayPos,
        focus_ring: bool,
        layer: RenderLayer,
        push: &mut dyn FnMut(WorkspaceRenderElement<R>),
    ) {
        let scrolling_focus_ring = focus_ring && !self.floating_is_active();
        self.tiling
            .render(ctx, xray_pos, scrolling_focus_ring, layer, &mut |elem| {
                push(elem.into())
            });
    }

    pub fn render_floating<R: NiriRenderer>(
        &self,
        ctx: RenderCtx<R>,
        xray_pos: XrayPos,
        focus_ring: bool,
        layer: RenderLayer,
        push: &mut dyn FnMut(WorkspaceRenderElement<R>),
    ) {
        if !self.is_floating_visible() && layer.is_normal() {
            return;
        }

        let view_rect = Rectangle::from_size(self.view_size);
        let floating_focus_ring = focus_ring && self.floating_is_active();
        self.floating.render(
            ctx,
            xray_pos,
            view_rect,
            floating_focus_ring,
            layer,
            &mut |elem| push(elem.into()),
        );
    }

    pub fn render_shadow<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        push: &mut dyn FnMut(ShadowRenderElement),
    ) {
        self.shadow.render(renderer, Point::from((0., 0.)), push);
    }

    pub fn render_background(&self) -> SolidColorRenderElement {
        SolidColorRenderElement::from_buffer(
            &self.background_buffer,
            Point::new(0., 0.),
            1.,
            Kind::Unspecified,
        )
    }

    pub fn render_above_top_layer(&self) -> bool {
        self.tiling.render_above_top_layer()
    }

    pub fn is_floating_visible(&self) -> bool {
        // If focus is on a fullscreen tiled window, hide the floating windows.
        matches!(
            self.floating_is_active,
            FloatingActive::Yes | FloatingActive::NoButRaised
        ) || !self.render_above_top_layer()
    }
}

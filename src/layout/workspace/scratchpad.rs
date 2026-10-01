//! Preparing windows to enter and leave the scratchpad.

use super::*;

impl<W: LayoutElement> Workspace<W> {
    pub(in crate::layout) fn prepare_tiled_window_for_scratchpad(
        &mut self,
        id: &W::Id,
        automatic_maximum: Size<i32, Logical>,
    ) {
        let Some(tile) = self
            .tiling
            .tiles_mut()
            .find(|tile| tile.window().id() == id)
        else {
            return;
        };

        // Sway sizes a tiled view from the workspace box when it first enters
        // the scratchpad, and sizes its content, not the decorated container:
        // container_floating_set_default_size sets content_width/height and
        // derives the geometry from them (sway/tree/container.c:896-918).
        let (minimum, maximum) = crate::layout::floating_tree::floating_constraints(
            self.options.layout.floating_minimum_size,
            self.options.layout.floating_maximum_size,
            automatic_maximum.to_f64(),
        );
        let content_width = (self.working_area.size.w * 0.5)
            .min(maximum.w)
            .max(minimum.w);
        let content_height = (self.working_area.size.h * 0.75)
            .min(maximum.h)
            .max(minimum.h);
        let min_size = tile.window().min_size();
        let max_size = tile.window().max_size();
        let window_width =
            ensure_min_max_size(content_width.round() as i32, min_size.w, max_size.w);
        let window_height =
            ensure_min_max_size(content_height.round() as i32, min_size.h, max_size.h);
        tile.floating_window_size = Some(Size::from((window_width.max(1), window_height.max(1))));

        let tile_size = Size::from((
            tile.tile_width_for_window_width(f64::from(window_width)),
            tile.tile_height_for_window_height(f64::from(window_height)),
        ));
        let pos = self.working_area.loc
            + (self.working_area.size.to_point() - tile_size.to_point()).downscale(2.);
        tile.floating_pos = Some(self.floating.logical_to_size_frac(pos));
    }

    pub fn remap_floating_position(
        &self,
        tile: &mut Tile<W>,
        old_area: Option<Rectangle<f64, Logical>>,
    ) {
        self.floating.remap_stored_tile_pos(tile, old_area);
    }
}

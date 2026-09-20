use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI};

use pangocairo::cairo::{self, ImageSurface};
use pangocairo::pango::{EllipsizeMode, FontDescription};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::GlesTexture;
use smithay::utils::{Logical, Rectangle, Transform};

use super::tiling_tree::NodeId;
use crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::texture::{TextureBuffer, TextureRenderElement};
use crate::utils::to_physical_precise_round;

#[derive(Debug, Clone)]
pub struct Titlebar<I> {
    pub target: I,
    pub rect: Rectangle<f64, Logical>,
    pub ipc_rect: Rectangle<f64, Logical>,
    pub title: String,
    pub state: TitlebarState,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitlebarState {
    Focused,
    FocusedInactive,
    FocusedTabTitle,
    Unfocused,
    Urgent,
}

#[derive(Debug)]
struct CachedTitlebar {
    title: String,
    width: i32,
    height: i32,
    scale: f64,
    state: TitlebarState,
    config: swayward_config::Titlebar,
    /// Top-left and top-right radius in physical pixels.
    top_radius: (f64, f64),
    buffer: TextureBuffer<GlesTexture>,
}

#[derive(Debug, Default)]
pub struct TitlebarRenderer {
    buffers: RefCell<HashMap<NodeId, CachedTitlebar>>,
}

/// Text height used instead of a real pango measurement under test.
///
/// `height` lays out real glyphs, so its result depends on which fonts the
/// machine has: this developer container measures 14px for "Mg" at monospace 10
/// and the CI runner measures 13. That one pixel propagated into every geometry
/// snapshot and made the suite unrunnable anywhere but the author's machine.
/// Tests assert layout arithmetic, not font rasterisation, so they get a fixed
/// value; `titlebar::height` still measures for real at runtime.
#[cfg(test)]
const TEST_TEXT_HEIGHT: i32 = 14;

#[cfg(test)]
pub fn height(scale: f64, config: &swayward_config::Titlebar) -> f64 {
    f64::from(TEST_TEXT_HEIGHT) / scale + config.vertical_padding * 2.
}

#[cfg(not(test))]
pub fn height(scale: f64, config: &swayward_config::Titlebar) -> f64 {
    height_measured(scale, config)
}

#[cfg(not(test))]
fn height_measured(scale: f64, config: &swayward_config::Titlebar) -> f64 {
    let measured = ImageSurface::create(cairo::Format::ARgb32, 1, 1)
        .ok()
        .and_then(|surface| cairo::Context::new(&surface).ok())
        .map(|cr| {
            let layout = pangocairo::functions::create_layout(&cr);
            let mut font = FontDescription::from_string(&config.font);
            font.set_absolute_size(to_physical_precise_round(scale, font.size()));
            layout.set_font_description(Some(&font));
            layout.set_text("Mg");
            layout.pixel_size().1
        })
        .unwrap_or(14);
    f64::from(measured) / scale + config.vertical_padding * 2.
}

pub(crate) fn physical_extent(scale: f64, offset: f64, length: f64) -> i32 {
    to_physical_precise_round::<i32>(scale, offset + length)
        - to_physical_precise_round::<i32>(scale, offset)
}

impl TitlebarRenderer {
    pub fn retain(&self, ids: impl Iterator<Item = NodeId>) {
        let ids = ids.collect::<Vec<_>>();
        self.buffers.borrow_mut().retain(|id, _| ids.contains(id));
    }

    pub fn render<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        id: NodeId,
        titlebar: &Titlebar<impl Clone>,
        scale: f64,
        config: &swayward_config::Titlebar,
        top_radius: (f64, f64),
    ) -> Option<PrimaryGpuTextureRenderElement> {
        let width = physical_extent(scale, titlebar.rect.loc.x, titlebar.rect.size.w).max(1);
        let height = physical_extent(scale, titlebar.rect.loc.y, titlebar.rect.size.h).max(1);
        // A titlebar sits directly on top of the window, so its top corners
        // must match the window's or the tab bar shows square shoulders above
        // a rounded frame. Scale to physical pixels and clamp so a large
        // radius on a short titlebar cannot produce a malformed arc.
        let max_radius = f64::from(width.min(height * 2)) / 2.;
        let top_radius = (
            (scale * top_radius.0).clamp(0., max_radius),
            (scale * top_radius.1).clamp(0., max_radius),
        );
        let mut buffers = self.buffers.borrow_mut();
        let reusable = buffers.get(&id).is_some_and(|cached| {
            cached.title == titlebar.title
                && cached.width == width
                && cached.height == height
                && cached.scale == scale
                && cached.state == titlebar.state
                && cached.config == *config
                && cached.top_radius == top_radius
        });
        if !reusable {
            let buffer =
                render_buffer(renderer, titlebar, scale, width, height, config, top_radius)?;
            buffers.insert(
                id,
                CachedTitlebar {
                    title: titlebar.title.clone(),
                    width,
                    height,
                    scale,
                    state: titlebar.state,
                    config: config.clone(),
                    top_radius,
                    buffer,
                },
            );
        }
        let buffer = buffers.get(&id)?.buffer.clone();
        Some(PrimaryGpuTextureRenderElement(
            TextureRenderElement::from_texture_buffer(
                buffer,
                titlebar.rect.loc,
                1.,
                None,
                None,
                Kind::Unspecified,
            ),
        ))
    }
}

fn render_buffer<R: NiriRenderer>(
    renderer: &mut R,
    titlebar: &Titlebar<impl Clone>,
    scale: f64,
    width: i32,
    height: i32,
    config: &swayward_config::Titlebar,
    top_radius: (f64, f64),
) -> Option<TextureBuffer<GlesTexture>> {
    let surface = ImageSurface::create(cairo::Format::ARgb32, width, height).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;
    let colors = match titlebar.state {
        TitlebarState::Focused => config.focused,
        TitlebarState::FocusedInactive => config.focused_inactive,
        TitlebarState::FocusedTabTitle => config.focused_tab_title,
        TitlebarState::Unfocused => config.unfocused,
        TitlebarState::Urgent => config.urgent,
    };
    let [r, g, b, a] = colors.background_color.to_array_unpremul();
    cr.set_source_rgba(r.into(), g.into(), b.into(), a.into());
    let (tl, tr) = top_radius;
    if tl <= 0. && tr <= 0. {
        cr.paint().ok()?;
    } else {
        // Round only the top corners: the bottom edge meets the window.
        let (w, h) = (f64::from(width), f64::from(height));
        cr.new_sub_path();
        cr.arc(w - tr, tr, tr, -FRAC_PI_2, 0.);
        cr.line_to(w, h);
        cr.line_to(0., h);
        cr.arc(tl, tl, tl, PI, 3. * FRAC_PI_2);
        cr.close_path();
        cr.fill().ok()?;
    }

    let layout = pangocairo::functions::create_layout(&cr);
    layout.context().set_round_glyph_positions(false);
    let mut font = FontDescription::from_string(&config.font);
    font.set_absolute_size(to_physical_precise_round(scale, font.size()));
    layout.set_font_description(Some(&font));
    layout.set_ellipsize(EllipsizeMode::End);
    let horizontal_padding = to_physical_precise_round::<i32>(scale, config.horizontal_padding);
    layout.set_width((width - horizontal_padding * 2).max(1) * pangocairo::pango::SCALE);
    layout.set_text(&titlebar.title);
    let (_, text_height) = layout.pixel_size();
    cr.move_to(
        f64::from(horizontal_padding),
        f64::from((height - text_height).max(0)) / 2.,
    );
    let [r, g, b, a] = colors.text_color.to_array_unpremul();
    cr.set_source_rgba(r.into(), g.into(), b.into(), a.into());
    pangocairo::functions::show_layout(&cr, &layout);
    drop(cr);

    let data = surface.take_data().ok()?;
    TextureBuffer::from_memory(
        renderer.as_gles_renderer(),
        &data,
        Fourcc::Argb8888,
        (width, height),
        false,
        scale,
        Transform::Normal,
        Vec::new(),
    )
    .ok()
}

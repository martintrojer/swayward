use std::cell::RefCell;
use std::collections::HashMap;

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

const FONT: &str = "monospace 10";
const H_PADDING: i32 = 5;
const V_PADDING: i32 = 4;
const ACTIVE: (f64, f64, f64) = (0.28, 0.46, 0.64);
const INACTIVE: (f64, f64, f64) = (0.16, 0.16, 0.16);

#[derive(Debug, Clone)]
pub struct Titlebar<I> {
    pub target: I,
    pub rect: Rectangle<f64, Logical>,
    pub ipc_rect: Rectangle<f64, Logical>,
    pub title: String,
    pub active: bool,
    pub visible: bool,
}

#[derive(Debug)]
struct CachedTitlebar {
    title: String,
    width: i32,
    height: i32,
    scale: f64,
    active: bool,
    buffer: TextureBuffer<GlesTexture>,
}

#[derive(Debug, Default)]
pub struct TitlebarRenderer {
    buffers: RefCell<HashMap<NodeId, CachedTitlebar>>,
}

pub fn height(scale: f64) -> f64 {
    let measured = ImageSurface::create(cairo::Format::ARgb32, 1, 1)
        .ok()
        .and_then(|surface| cairo::Context::new(&surface).ok())
        .map(|cr| {
            let layout = pangocairo::functions::create_layout(&cr);
            let mut font = FontDescription::from_string(FONT);
            font.set_absolute_size(to_physical_precise_round(scale, font.size()));
            layout.set_font_description(Some(&font));
            layout.set_text("Mg");
            layout.pixel_size().1
        })
        .unwrap_or(14);
    f64::from(measured + V_PADDING * 2) / scale
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
    ) -> Option<PrimaryGpuTextureRenderElement> {
        let width = to_physical_precise_round::<i32>(scale, titlebar.rect.size.w).max(1);
        let height = to_physical_precise_round::<i32>(scale, titlebar.rect.size.h).max(1);
        let mut buffers = self.buffers.borrow_mut();
        let reusable = buffers.get(&id).is_some_and(|cached| {
            cached.title == titlebar.title
                && cached.width == width
                && cached.height == height
                && cached.scale == scale
                && cached.active == titlebar.active
        });
        if !reusable {
            let buffer = render_buffer(renderer, titlebar, scale, width, height)?;
            buffers.insert(
                id,
                CachedTitlebar {
                    title: titlebar.title.clone(),
                    width,
                    height,
                    scale,
                    active: titlebar.active,
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
) -> Option<TextureBuffer<GlesTexture>> {
    let surface = ImageSurface::create(cairo::Format::ARgb32, width, height).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;
    let (r, g, b) = if titlebar.active { ACTIVE } else { INACTIVE };
    cr.set_source_rgb(r, g, b);
    cr.paint().ok()?;

    let layout = pangocairo::functions::create_layout(&cr);
    layout.context().set_round_glyph_positions(false);
    let mut font = FontDescription::from_string(FONT);
    font.set_absolute_size(to_physical_precise_round(scale, font.size()));
    layout.set_font_description(Some(&font));
    layout.set_ellipsize(EllipsizeMode::End);
    layout.set_width((width - H_PADDING * 2).max(1) * pangocairo::pango::SCALE);
    layout.set_text(&titlebar.title);
    let (_, text_height) = layout.pixel_size();
    cr.move_to(
        f64::from(H_PADDING),
        f64::from((height - text_height).max(0)) / 2.,
    );
    cr.set_source_rgb(1., 1., 1.);
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

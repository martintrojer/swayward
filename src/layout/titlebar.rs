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
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};
use crate::render_helpers::texture::{TextureBuffer, TextureRenderElement};
use crate::utils::to_physical_precise_round;

#[derive(Debug, Clone)]
pub struct Titlebar<I> {
    pub target: I,
    pub rect: Rectangle<f64, Logical>,
    pub ipc_rect: Rectangle<f64, Logical>,
    pub title: String,
    pub marks: Vec<String>,
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
    marks: Vec<String>,
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
    uncovered_top_borders: RefCell<HashMap<(NodeId, usize), SolidColorBuffer>>,
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
        self.uncovered_top_borders
            .borrow_mut()
            .retain(|(id, _), _| ids.contains(id));
    }

    pub fn render_uncovered_top_border(
        &self,
        id: NodeId,
        index: usize,
        rect: Rectangle<f64, Logical>,
        config: swayward_config::FocusRing,
        is_active: bool,
        is_urgent: bool,
    ) -> Option<SolidColorRenderElement> {
        if config.off {
            return None;
        }
        let color = if is_urgent {
            config.urgent_color
        } else if is_active {
            config.active_color
        } else {
            config.inactive_color
        };
        let mut borders = self.uncovered_top_borders.borrow_mut();
        let border = borders.entry((id, index)).or_default();
        border.update(rect.size, color);
        Some(SolidColorRenderElement::from_buffer(
            border,
            rect.loc,
            1.,
            Kind::Unspecified,
        ))
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
        // The decorated-box model assigns each titlebar its outer top corners.
        // Scale those owned radii to physical pixels and clamp so a large
        // radius on a short titlebar cannot produce a malformed arc.
        let max_radius = f64::from(width.min(height * 2)) / 2.;
        let top_radius = (
            (scale * top_radius.0).clamp(0., max_radius),
            (scale * top_radius.1).clamp(0., max_radius),
        );
        let mut buffers = self.buffers.borrow_mut();
        let reusable = buffers.get(&id).is_some_and(|cached| {
            cached.title == titlebar.title
                && cached.marks == titlebar.marks
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
                    marks: titlebar.marks.clone(),
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
    let surface = paint_titlebar(titlebar, scale, width, height, config, top_radius)?;

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

/// Paint a titlebar into a CPU surface: background, border ring and title.
///
/// Split from the GPU upload so the ring, which sway draws as full rect minus an
/// inset background (`sway/tree/container.c:352-368`), can be asserted on
/// pixels without a renderer.
fn paint_titlebar(
    titlebar: &Titlebar<impl Clone>,
    scale: f64,
    width: i32,
    height: i32,
    config: &swayward_config::Titlebar,
    top_radius: (f64, f64),
) -> Option<ImageSurface> {
    let surface = ImageSurface::create(cairo::Format::ARgb32, width, height).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;
    let colors = match titlebar.state {
        TitlebarState::Focused => config.focused,
        TitlebarState::FocusedInactive => config.focused_inactive,
        TitlebarState::FocusedTabTitle => config.focused_tab_title,
        TitlebarState::Unfocused => config.unfocused,
        TitlebarState::Urgent => config.urgent,
    };
    let thickness = scale * f64::from(config.border_thickness);
    let (physical_width, physical_height) = (f64::from(width), f64::from(height));
    let (tl, tr) = top_radius;

    let outer_color = if thickness > 0. {
        colors.border_color
    } else {
        colors.background_color
    };
    let [r, g, b, a] = outer_color.to_array_unpremul();
    cr.set_source_rgba(r.into(), g.into(), b.into(), a.into());
    rounded_titlebar_path(&cr, 0., 0., physical_width, physical_height, tl, tr);
    cr.fill().ok()?;

    if thickness > 0. && physical_width > thickness * 2. && physical_height > thickness * 2. {
        let [r, g, b, a] = colors.background_color.to_array_unpremul();
        cr.set_source_rgba(r.into(), g.into(), b.into(), a.into());
        rounded_titlebar_path(
            &cr,
            thickness,
            thickness,
            physical_width - thickness * 2.,
            physical_height - thickness * 2.,
            (tl - thickness).max(0.),
            (tr - thickness).max(0.),
        );
        cr.fill().ok()?;
    }

    let layout = pangocairo::functions::create_layout(&cr);
    layout.context().set_round_glyph_positions(false);
    let mut font = FontDescription::from_string(&config.font);
    font.set_absolute_size(to_physical_precise_round(scale, font.size()));
    layout.set_font_description(Some(&font));
    layout.set_ellipsize(EllipsizeMode::End);
    let horizontal_padding = to_physical_precise_round::<i32>(scale, config.horizontal_padding);
    let has_visible_marks =
        config.show_marks && titlebar.marks.iter().any(|mark| !mark.starts_with('_'));
    let text = titlebar_text(titlebar, config);
    layout.set_width((width - horizontal_padding * 2).max(1) * pangocairo::pango::SCALE);
    if config.pango_markup && !has_visible_marks {
        layout.set_markup(&titlebar.title);
    } else {
        layout.set_text(&text);
    }
    let (text_width, text_height) = layout.pixel_size();
    let x = match config.alignment {
        swayward_config::TitleAlignment::Left => horizontal_padding,
        swayward_config::TitleAlignment::Center => (width - text_width) / 2,
        swayward_config::TitleAlignment::Right => width - horizontal_padding - text_width,
    }
    .max(horizontal_padding);
    cr.move_to(f64::from(x), f64::from((height - text_height).max(0)) / 2.);
    let [r, g, b, a] = colors.text_color.to_array_unpremul();
    cr.set_source_rgba(r.into(), g.into(), b.into(), a.into());
    pangocairo::functions::show_layout(&cr, &layout);
    drop(cr);
    Some(surface)
}

fn titlebar_text(titlebar: &Titlebar<impl Clone>, config: &swayward_config::Titlebar) -> String {
    let marks = if config.show_marks {
        titlebar
            .marks
            .iter()
            .filter(|mark| !mark.starts_with('_'))
            .map(|mark| format!("[{mark}]"))
            .collect::<String>()
    } else {
        String::new()
    };
    if config.alignment == swayward_config::TitleAlignment::Right {
        format!("{marks}{}", titlebar.title)
    } else {
        format!("{}{marks}", titlebar.title)
    }
}

fn rounded_titlebar_path(
    cr: &cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    top_left: f64,
    top_right: f64,
) {
    cr.new_sub_path();
    cr.move_to(x + top_left, y);
    cr.line_to(x + width - top_right, y);
    if top_right > 0. {
        cr.arc(
            x + width - top_right,
            y + top_right,
            top_right,
            -FRAC_PI_2,
            0.,
        );
    }
    cr.line_to(x + width, y + height);
    cr.line_to(x, y + height);
    if top_left > 0. {
        cr.arc(x + top_left, y + top_left, top_left, PI, 3. * FRAC_PI_2);
    }
    cr.close_path();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(surface: &mut ImageSurface, x: i32, y: i32) -> [u8; 4] {
        let stride = surface.stride();
        surface.flush();
        let data = surface.data().unwrap();
        let i = (y * stride + x * 4) as usize;
        // Cairo ARGB32 is native-endian; on little-endian that is B, G, R, A.
        [data[i + 2], data[i + 1], data[i], data[i + 3]]
    }

    fn rgb(color: swayward_config::Color) -> [u8; 3] {
        let [r, g, b, _] = color.to_array_unpremul();
        [r, g, b].map(|c| (c * 255.).round() as u8)
    }

    #[test]
    fn titlebar_marks_follow_sway_visibility_and_alignment() {
        let titlebar = Titlebar {
            target: (),
            rect: Rectangle::default(),
            ipc_rect: Rectangle::default(),
            title: "term".into(),
            marks: vec!["work".into(), "_private".into()],
            state: TitlebarState::Focused,
            visible: true,
        };
        let mut config = swayward_config::Titlebar::default();
        assert_eq!(titlebar_text(&titlebar, &config), "term[work]");
        config.alignment = swayward_config::TitleAlignment::Right;
        assert_eq!(titlebar_text(&titlebar, &config), "[work]term");
        config.show_marks = false;
        assert_eq!(titlebar_text(&titlebar, &config), "term");
    }

    #[test]
    fn titlebar_draws_an_inset_border_ring_in_the_border_colour() {
        // Sway draws a titlebar as full rect minus an inset background, the
        // ring filled with the class border colour
        // (sway/tree/container.c:352-368). Removing the ring left every other
        // test green, so assert it on pixels.
        let mut config = swayward_config::Titlebar {
            border_thickness: 3,
            ..Default::default()
        };
        config.focused.border_color = swayward_config::Color::new_unpremul(1., 0., 0., 1.);
        config.focused.background_color = swayward_config::Color::new_unpremul(0., 0., 1., 1.);
        let titlebar = Titlebar {
            target: (),
            rect: Rectangle::from_size((200., 30.).into()),
            ipc_rect: Rectangle::from_size((200., 30.).into()),
            title: String::new(),
            marks: Vec::new(),
            state: TitlebarState::Focused,
            visible: true,
        };
        let mut surface = paint_titlebar(&titlebar, 1., 200, 30, &config, (0., 0.)).unwrap();
        let border = rgb(config.focused.border_color);
        let background = rgb(config.focused.background_color);

        // Every edge carries the ring, and it is exactly `thickness` deep.
        for (x, y) in [
            (100, 0),
            (100, 2),
            (100, 29),
            (100, 27),
            (0, 15),
            (2, 15),
            (199, 15),
            (197, 15),
        ] {
            assert_eq!(pixel(&mut surface, x, y)[..3], border, "ring at ({x}, {y})");
        }
        for (x, y) in [(100, 3), (100, 26), (3, 15), (196, 15), (100, 15)] {
            assert_eq!(
                pixel(&mut surface, x, y)[..3],
                background,
                "background at ({x}, {y})"
            );
        }

        // Zero thickness draws no ring, matching sway.
        config.border_thickness = 0;
        let mut surface = paint_titlebar(&titlebar, 1., 200, 30, &config, (0., 0.)).unwrap();
        assert_eq!(pixel(&mut surface, 100, 0)[..3], background);
    }
}

use anyhow::{Context, Result};
use hayro::RenderCache;
use hayro::RenderSettings;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use image::{DynamicImage, RgbaImage};

/// Render using a caller-provided cache for reuse across multiple pages
/// of the same document.
pub fn render_page_with_cache<'a>(
    pdf: &'a Pdf,
    page_idx: usize,
    scale: f32,
    max_width: Option<u16>,
    max_height: Option<u16>,
    cache: &RenderCache<'a>,
) -> Result<DynamicImage> {
    let interp = InterpreterSettings::default();
    let pages = pdf.pages();
    let page = pages
        .get(page_idx)
        .with_context(|| format!("page {page_idx} out of range"))?;

    // Compute scale to fill the available area (maintaining aspect ratio).
    // Use at least `scale` as a minimum quality floor.
    let (page_w, page_h) = page.render_dimensions();
    let effective_scale = if let (Some(mw), Some(mh)) = (max_width, max_height) {
        let sw = mw as f32 / page_w;
        let sh = mh as f32 / page_h;
        // Fit within max dims (use smaller ratio), but don't go below base scale
        sw.min(sh).max(scale)
    } else {
        scale
    };

    let settings = RenderSettings {
        x_scale: effective_scale,
        y_scale: effective_scale,
        bg_color: WHITE,
        ..Default::default()
    };

    let pixmap = hayro::render(page, cache, &interp, &settings);
    pixmap_to_image(pixmap)
}

/// Convert a vello_cpu Pixmap (premultiplied RGBA) to an image::DynamicImage.
fn pixmap_to_image(pixmap: hayro::vello_cpu::Pixmap) -> Result<DynamicImage> {
    let w = pixmap.width() as u32;
    let h = pixmap.height() as u32;

    let data = pixmap.data();
    let mut rgba_buf = Vec::with_capacity((w * h * 4) as usize);
    for px in data {
        if px.a == 0 {
            rgba_buf.extend_from_slice(&[255, 255, 255, 255]);
        } else if px.a == 255 {
            rgba_buf.extend_from_slice(&[px.r, px.g, px.b, 255]);
        } else {
            let inv = 255.0 / px.a as f32;
            rgba_buf.extend_from_slice(&[
                (px.r as f32 * inv).min(255.0) as u8,
                (px.g as f32 * inv).min(255.0) as u8,
                (px.b as f32 * inv).min(255.0) as u8,
                px.a,
            ]);
        }
    }

    let img = RgbaImage::from_raw(w, h, rgba_buf)
        .context("failed to create image from pixmap data")?;
    Ok(DynamicImage::ImageRgba8(img))
}

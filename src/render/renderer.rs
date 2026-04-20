use anyhow::{Context, Result};
use hayro::RenderCache;
use hayro::RenderSettings;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use image::{DynamicImage, RgbaImage};

/// Returns current RSS in MB by querying `ps`. Used for debug logging.
fn rss_mb() -> f64 {
    let pid = std::process::id();
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|kb| kb as f64 / 1024.0)
        .unwrap_or(0.0)
}

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
    // Use at least `scale` as a minimum quality floor, and cap at 3.0 to
    // prevent enormous pixmaps on high-res terminals.
    let (page_w, page_h) = page.render_dimensions();
    let effective_scale = if let (Some(mw), Some(mh)) = (max_width, max_height) {
        let sw = mw as f32 / page_w;
        let sh = mh as f32 / page_h;
        // Fit within max dims (use smaller ratio), floor at base scale, cap at 3.0
        sw.min(sh).max(scale).min(3.0)
    } else {
        scale
    };

    let settings = RenderSettings {
        x_scale: effective_scale,
        y_scale: effective_scale,
        bg_color: WHITE,
        ..Default::default()
    };

    let pre_rss = rss_mb();
    let pixmap = hayro::render(page, cache, &interp, &settings);
    let post_rss = rss_mb();
    let pixmap_mb = pixmap.width() as f64 * pixmap.height() as f64 * 4.0 / 1_048_576.0;
    eprintln!(
        "[render] page={page_idx} scale={effective_scale:.2} pdf={page_w:.0}x{page_h:.0} \
         pixmap={}x{} ({pixmap_mb:.1}MB) rss={pre_rss:.0}→{post_rss:.0}MB (Δ{:.0}MB)",
        pixmap.width(),
        pixmap.height(),
        post_rss - pre_rss,
    );

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

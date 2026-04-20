use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("usage: bench_render <pdf>");
    let bytes = std::fs::read(&path).expect("failed to read file");

    let t0 = Instant::now();
    let pdf = hayro::hayro_syntax::Pdf::new(bytes).expect("failed to parse PDF");
    println!("PDF parse: {}ms", t0.elapsed().as_millis());
    println!("Pages: {}", pdf.pages().len());

    let pages = pdf.pages();
    let page = &pages[0];
    let (pw, ph) = page.render_dimensions();
    println!("Page 0 dimensions: {pw:.0} x {ph:.0} pt");

    // Benchmark hayro render at different scales
    for (label, scale, max_w, max_h) in [
        ("full (1.0)", 1.0f32, None, None),
        ("capped 400px", 2.0, Some(400u16), Some(600u16)),
        ("capped 200px", 2.0, Some(200), Some(300)),
        ("thumbnail", 0.15, None, None),
        ("thumb capped 128px", 0.5, Some(128), Some(128)),
    ] {
        let cache = hayro::RenderCache::new();
        let interp = hayro::hayro_interpret::InterpreterSettings::default();

        let effective_scale = if let (Some(mw), Some(mh)) = (max_w, max_h) {
            let sw = mw as f32 / pw;
            let sh = mh as f32 / ph;
            scale.min(sw).min(sh)
        } else {
            scale
        };

        let settings = hayro::RenderSettings {
            x_scale: effective_scale,
            y_scale: effective_scale,
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
            ..Default::default()
        };

        let t1 = Instant::now();
        let pixmap = hayro::render(page, &cache, &interp, &settings);
        let render_ms = t1.elapsed().as_millis();

        let w = pixmap.width();
        let h = pixmap.height();

        // Benchmark pixmap -> image conversion
        let t2 = Instant::now();
        let data = pixmap.data();
        let mut rgba_buf = Vec::with_capacity((w as usize) * (h as usize) * 4);
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
        let _img = image::RgbaImage::from_raw(w as u32, h as u32, rgba_buf);
        let convert_ms = t2.elapsed().as_millis();

        // Benchmark protocol encoding
        let img2 = {
            let data2 = pixmap.data();
            let mut buf = Vec::with_capacity((w as usize) * (h as usize) * 4);
            for px in data2 {
                if px.a == 255 {
                    buf.extend_from_slice(&[px.r, px.g, px.b, 255]);
                } else {
                    buf.extend_from_slice(&[255, 255, 255, 255]);
                }
            }
            image::DynamicImage::ImageRgba8(
                image::RgbaImage::from_raw(w as u32, h as u32, buf).unwrap(),
            )
        };

        let picker = ratatui_image::picker::Picker::from_fontsize((8, 16));
        let t3 = Instant::now();
        let _proto = picker.new_resize_protocol(img2);
        let proto_ms = t3.elapsed().as_millis();

        println!(
            "  {label:<22} scale={effective_scale:.3} {w}x{h}px  hayro={render_ms}ms  convert={convert_ms}ms  protocol={proto_ms}ms"
        );
    }

    // Benchmark rendering 5 thumbnails with shared cache
    println!("\nBatch thumbnail render (5 pages, shared cache):");
    let cache = hayro::RenderCache::new();
    let interp = hayro::hayro_interpret::InterpreterSettings::default();
    let t_batch = Instant::now();
    let count = 5.min(pages.len());
    for i in 0..count {
        let page = &pages[i];
        let (pw, ph) = page.render_dimensions();
        let s = (128.0 / ph).min(128.0 / pw).min(0.5);
        let settings = hayro::RenderSettings {
            x_scale: s,
            y_scale: s,
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
            ..Default::default()
        };
        let _pixmap = hayro::render(page, &cache, &interp, &settings);
    }
    let batch_ms = t_batch.elapsed().as_millis();
    println!("  {count} thumbnails with shared cache: {batch_ms}ms ({:.0}ms/page)", batch_ms as f64 / count as f64);

    // Same without shared cache
    let t_batch2 = Instant::now();
    for i in 0..count {
        let cache2 = hayro::RenderCache::new();
        let page = &pages[i];
        let (pw, ph) = page.render_dimensions();
        let s = (128.0 / ph).min(128.0 / pw).min(0.5);
        let settings = hayro::RenderSettings {
            x_scale: s,
            y_scale: s,
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
            ..Default::default()
        };
        let _pixmap = hayro::render(page, &cache2, &interp, &settings);
    }
    let batch2_ms = t_batch2.elapsed().as_millis();
    println!("  {count} thumbnails with NEW cache each: {batch2_ms}ms ({:.0}ms/page)", batch2_ms as f64 / count as f64);
}

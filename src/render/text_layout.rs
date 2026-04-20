use anyhow::{Context as _, Result};
use hayro::hayro_interpret::font::Glyph;
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache,
    InterpreterSettings, Paint, PathDrawMode, SoftMask, interpret_page,
};
use hayro::hayro_syntax::Pdf;
use kurbo::{Affine, BezPath, Point, Rect};

/// A positioned character extracted from a PDF page.
struct PlacedChar {
    x: f64,
    y: f64,
    /// Expected x of the next same-word glyph (from advance width).
    next_x: f64,
    ch: String,
}

/// Device that collects positioned Unicode characters.
struct TextExtractor {
    chars: Vec<PlacedChar>,
    page_height: f64,
}

impl TextExtractor {
    fn new(dimensions: (f32, f32)) -> Self {
        Self {
            chars: Vec::new(),
            page_height: dimensions.1 as f64,
        }
    }
}

impl Device<'_> for TextExtractor {
    fn set_soft_mask(&mut self, _: Option<SoftMask<'_>>) {}
    fn set_blend_mode(&mut self, _: BlendMode) {}
    fn draw_path(&mut self, _: &BezPath, _: Affine, _: &Paint<'_>, _: &PathDrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
    fn pop_clip_path(&mut self) {}
    fn pop_transparency_group(&mut self) {}
    fn draw_image(&mut self, _: Image<'_, '_>, _: Affine) {}

    fn draw_glyph(
        &mut self,
        glyph: &Glyph<'_>,
        transform: Affine,
        glyph_transform: Affine,
        _: &Paint<'_>,
        _: &GlyphDrawMode,
    ) {
        if let Some(unicode) = glyph.as_unicode() {
            let s = match unicode {
                BfString::Char(c) => {
                    if c.is_control() {
                        return;
                    }
                    c.to_string()
                }
                BfString::String(s) => s,
            };
            if s.is_empty() {
                return;
            }

            let pos = (transform * glyph_transform) * Point::ZERO;

            let advance_pts = match glyph {
                Glyph::Outline(og) => og.advance_width().map(|aw| {
                    let combined = transform * glyph_transform;
                    let origin = combined * Point::ZERO;
                    let advanced = combined * Point::new(aw as f64, 0.0);
                    advanced.x - origin.x
                }),
                _ => None,
            };

            let next_x = match advance_pts {
                Some(adv) if adv > 0.0 => pos.x + adv,
                _ => pos.x,
            };

            self.chars.push(PlacedChar {
                x: pos.x,
                y: self.page_height - pos.y,
                next_x,
                ch: s,
            });
        }
    }
}

fn extract_chars(pdf: &Pdf, page_idx: usize) -> Result<(Vec<PlacedChar>, (f32, f32))> {
    let pages = pdf.pages();
    let page = pages
        .get(page_idx)
        .with_context(|| format!("page {page_idx} out of range"))?;

    let dims = page.render_dimensions();
    let mut extractor = TextExtractor::new(dims);

    let settings = InterpreterSettings::default();
    let cache = InterpreterCache::new();
    let mut context = Context::new(
        Affine::IDENTITY,
        Rect::new(0.0, 0.0, dims.0 as f64, dims.1 as f64),
        &cache,
        pdf.xref(),
        settings,
    );

    interpret_page(page, &mut context, &mut extractor);
    Ok((extractor.chars, dims))
}

/// Extract text from a PDF page and lay it out on a terminal grid.
pub fn extract_text_grid(pdf: &Pdf, page_idx: usize, cols: u16, rows: u16) -> Result<Vec<String>> {
    let (mut chars, dims) = extract_chars(pdf, page_idx)?;
    snap_y_to_grid(&mut chars);
    Ok(layout_to_grid(&chars, dims, cols, rows))
}

/// Find vertical positions of search matches on a page.
/// Returns a list of y-fractions [0.0, 1.0] where matches occur.
pub fn find_match_positions(pdf: &Pdf, page_idx: usize, query: &str) -> Result<Vec<f32>> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let (mut chars, dims) = extract_chars(pdf, page_idx)?;
    snap_y_to_grid(&mut chars);
    let lines = build_lines(&chars);
    if lines.is_empty() {
        return Ok(Vec::new());
    }

    let page_h = dims.1 as f64;
    let q_lower = query.to_lowercase();
    let mut positions = Vec::new();

    for line in &lines {
        let line_text: String = line.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        if line_text.to_lowercase().contains(&q_lower) {
            let frac = (line.y / page_h).clamp(0.0, 1.0) as f32;
            positions.push(frac);
        }
    }

    Ok(positions)
}

/// Detect the dominant line spacing and round all y-positions to that grid.
///
/// This merges glyphs that are at slightly different y-positions (e.g. line
/// numbers, superscripts, watermark chars) onto the same line as nearby
/// body text.
fn snap_y_to_grid(chars: &mut [PlacedChar]) {
    if chars.len() < 2 {
        return;
    }

    // Collect unique y-positions (rounded to 0.1pt to merge near-duplicates)
    let mut ys: Vec<f64> = chars.iter().map(|c| (c.y * 10.0).round() / 10.0).collect();
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ys.dedup();

    if ys.len() < 2 {
        return;
    }

    // Compute all gaps between consecutive unique y-positions
    let mut gaps: Vec<f64> = ys.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Find the dominant line spacing.
    // Bucket gaps by rounding to nearest 0.5pt, then find the smallest
    // bucket that has a significant number of gaps (>= 10% of total).
    // This gives us the most common "base" line spacing.
    let mut buckets: std::collections::BTreeMap<i32, usize> = std::collections::BTreeMap::new();
    for &g in &gaps {
        if g < 2.0 {
            continue; // ignore sub-2pt gaps (subscripts, kerning)
        }
        let key = (g * 2.0).round() as i32; // bucket to nearest 0.5pt
        *buckets.entry(key).or_insert(0) += 1;
    }

    let threshold = gaps.len() / 10; // at least 10% of gaps
    let best_spacing = buckets
        .iter()
        .filter(|&(_, count)| *count >= threshold.max(2))
        .map(|(&key, _)| key as f64 / 2.0)
        .next() // smallest bucket meeting threshold
        .unwrap_or(gaps[gaps.len() / 2]);

    if best_spacing < 1.0 {
        return;
    }

    // Find the best grid phase (baseline offset).
    // Try different offsets within [0, spacing) and pick the one that
    // captures the most glyphs "near" a grid line. This ensures the grid
    // aligns with the majority body text rather than sparse annotations.
    let n_phases = 12;
    let step = best_spacing / n_phases as f64;
    let mut best_phase = 0.0f64;
    let mut best_score = 0usize;

    for i in 0..n_phases {
        let phase = step * i as f64;
        let score: usize = chars
            .iter()
            .filter(|c| {
                let offset = (c.y - phase).rem_euclid(best_spacing);
                let dist = offset.min(best_spacing - offset);
                dist < best_spacing * 0.25
            })
            .count();
        if score > best_score {
            best_score = score;
            best_phase = phase;
        }
    }

    // Snap each glyph's y to the nearest grid line using the chosen phase
    for c in chars.iter_mut() {
        let offset = c.y - best_phase;
        let grid_idx = (offset / best_spacing).round();
        c.y = best_phase + grid_idx * best_spacing;
    }
}

/// Dump raw character positions for debugging.
#[allow(dead_code)]
pub fn dump_chars(pdf: &Pdf, page_idx: usize, limit: usize) -> Result<()> {
    let (chars, dims) = extract_chars(pdf, page_idx)?;
    eprintln!("page dims: {:.1} x {:.1}", dims.0, dims.1);
    eprintln!("{} glyphs extracted", chars.len());
    for (i, c) in chars.iter().enumerate().take(limit) {
        eprintln!(
            "  [{i:3}] ({:7.2}, {:7.2}) adv={:5.2} {:?}",
            c.x, c.y, c.next_x - c.x, c.ch
        );
    }
    Ok(())
}

/// Extract plain text from a PDF page.
pub fn extract_text(pdf: &Pdf, page_idx: usize) -> Result<String> {
    let (mut chars, _dims) = extract_chars(pdf, page_idx)?;
    chars.sort_by(|a, b| {
        a.y.partial_cmp(&b.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut result = String::new();
    for pc in &chars {
        result.push_str(&pc.ch);
    }
    Ok(result)
}

/// A word with its position.
struct Word {
    x: f64,
    text: String,
}

/// A line of text: a y-position and the words on it.
struct TextLine {
    y: f64,
    words: Vec<Word>,
    /// Median glyph advance for this line (determines char_size).
    char_advance: f64,
}

/// Group glyphs into words using advance-width prediction,
/// then cluster words into lines by y-position.
fn build_lines(chars: &[PlacedChar]) -> Vec<TextLine> {
    if chars.is_empty() {
        return Vec::new();
    }

    // Sort by y then x
    let mut sorted: Vec<usize> = (0..chars.len()).collect();
    sorted.sort_by(|&a, &b| {
        chars[a]
            .y
            .partial_cmp(&chars[b].y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                chars[a]
                    .x
                    .partial_cmp(&chars[b].x)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    // First pass: cluster into lines by y-position.
    // Glyphs within 2pt of the same y are on the same line.
    let mut line_groups: Vec<Vec<usize>> = Vec::new();
    let mut cur_line: Vec<usize> = vec![sorted[0]];
    let mut cur_y = chars[sorted[0]].y;

    for &idx in &sorted[1..] {
        if (chars[idx].y - cur_y).abs() < 2.0 {
            cur_line.push(idx);
        } else {
            line_groups.push(std::mem::take(&mut cur_line));
            cur_line.push(idx);
            cur_y = chars[idx].y;
        }
    }
    line_groups.push(cur_line);

    // Second pass: within each line, group into words using advance prediction.
    let mut lines: Vec<TextLine> = Vec::new();

    for group in &line_groups {
        let mut words: Vec<Word> = Vec::new();
        let mut advances: Vec<f64> = Vec::new();

        let first = &chars[group[0]];
        let mut word_x = first.x;
        let mut word_text = if first.ch.trim().is_empty() {
            String::new()
        } else {
            first.ch.clone()
        };
        let mut started = !first.ch.trim().is_empty();
        let mut prev = first;

        let adv = first.next_x - first.x;
        if adv > 0.1 && adv < 50.0 && !first.ch.trim().is_empty() {
            advances.push(adv);
        }

        for &idx in &group[1..] {
            let c = &chars[idx];
            let is_space = c.ch.trim().is_empty();

            let adv = c.next_x - c.x;
            if adv > 0.1 && adv < 50.0 && !is_space {
                advances.push(adv);
            }

            if is_space {
                flush_word(&mut words, &word_text, word_x);
                word_text.clear();
                started = false;
                prev = c;
                continue;
            }

            if !started {
                word_x = c.x;
                word_text = c.ch.clone();
                started = true;
                prev = c;
                continue;
            }

            // Word boundary detection: is the glyph placed beyond
            // where we'd expect the next same-word glyph?
            let predicted = prev.next_x;
            let actual = c.x;
            let prev_advance = (prev.next_x - prev.x).abs().max(1.0);
            let tolerance = (prev_advance * 0.30).max(1.0);

            if actual > predicted + tolerance {
                // Gap beyond advance — word boundary
                flush_word(&mut words, &word_text, word_x);
                word_x = c.x;
                word_text = c.ch.clone();
            } else if actual < prev.x - 1.0 {
                // Backwards — shouldn't happen within a line, but handle it
                flush_word(&mut words, &word_text, word_x);
                word_x = c.x;
                word_text = c.ch.clone();
            } else {
                word_text.push_str(&c.ch);
            }

            prev = c;
        }
        flush_word(&mut words, &word_text, word_x);

        if words.is_empty() {
            continue;
        }

        // Median advance for this line
        advances.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let char_advance = if !advances.is_empty() {
            advances[advances.len() / 2]
        } else {
            6.0
        };

        let y = chars[group[0]].y;
        lines.push(TextLine {
            y,
            words,
            char_advance,
        });
    }

    lines
}

fn flush_word(words: &mut Vec<Word>, text: &str, x: f64) {
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        words.push(Word {
            x,
            text: trimmed.to_string(),
        });
    }
}

/// Map text lines onto a terminal grid.
///
/// Uses the detected line spacing to decide blank rows: a y-gap of
/// more than 1.5× the line spacing produces 1 blank row; otherwise
/// lines are packed consecutively. This avoids excessive whitespace
/// from line numbers or other per-line annotations.
fn layout_to_grid(
    chars: &[PlacedChar],
    _page_dims: (f32, f32),
    cols: u16,
    _rows: u16,
) -> Vec<String> {
    if chars.is_empty() || cols == 0 {
        return Vec::new();
    }

    let lines = build_lines(chars);
    if lines.is_empty() {
        return Vec::new();
    }

    let cols = cols as usize;

    // Detect the dominant line spacing from the text lines themselves
    let line_spacing = detect_line_spacing(&lines);


    let mut output: Vec<String> = Vec::new();
    let mut prev_y: Option<f64> = None;

    for line in &lines {
        let blank_rows = if let Some(py) = prev_y {
            let gap = line.y - py;
            // > 1.5× line spacing → 1 blank row (paragraph/section break)
            // Otherwise → no blank row (consecutive lines)
            if gap > line_spacing * 1.5 {
                // Cap at 2 blank rows even for very large gaps
                ((gap / line_spacing).round() as usize).saturating_sub(1).min(2)
            } else {
                0
            }
        } else {
            0
        };

        for _ in 0..blank_rows {
            output.push(String::new());
        }

        let line_str = render_line(&line.words, line.char_advance, cols);
        output.push(line_str);
        prev_y = Some(line.y);
    }

    // Trim trailing empty lines
    while output.last().is_some_and(|l| l.is_empty()) {
        output.pop();
    }

    output
}

/// Detect the dominant line spacing from text line y-positions.
fn detect_line_spacing(lines: &[TextLine]) -> f64 {
    if lines.len() < 2 {
        return 12.0;
    }

    let mut gaps: Vec<f64> = lines
        .windows(2)
        .map(|w| w[1].y - w[0].y)
        .filter(|&g| g > 1.0)
        .collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    if gaps.is_empty() {
        return 12.0;
    }

    // The smallest common gap is the base line spacing.
    // Use the 25th percentile to be robust to occasional half-gaps.
    gaps[gaps.len() / 4]
}

/// Render a line's words into a string of at most `cols` characters.
/// Each word is placed at its x-position mapped via char_advance.
fn render_line(words: &[Word], char_advance: f64, cols: usize) -> String {
    let mut buf: Vec<char> = vec![' '; cols];
    let mut min_next = 0usize;

    for word in words {
        let col = (word.x / char_advance).round().max(0.0) as usize;
        // Ensure we don't overwrite previous word without a gap
        let col = col.max(min_next);
        let mut c = col;

        for ch in word.text.chars() {
            if c >= cols {
                break;
            }
            buf[c] = ch;
            c += 1;
        }
        min_next = c + 1; // leave at least one space after this word
    }

    let s: String = buf.into_iter().collect();
    s.trim_end().to_string()
}

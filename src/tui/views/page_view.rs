use crate::app::{App, Mode, ViewMode};
use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui_image::{Resize, StatefulImage};
use ratatui_image::protocol::StatefulProtocol;

/// State for the page view, holding the rendered image protocol.
pub struct PageViewState {
    pub protocol: Option<StatefulProtocol>,
    pub rendered_page: Option<usize>,
    /// Image dimensions (width, height) in pixels for centering.
    pub image_size: Option<(u32, u32)>,
    /// Cached text lines for the current page in text mode.
    pub text_lines: Option<(usize, Vec<String>)>,
    /// Vertical scroll offset for text view.
    pub text_scroll: u16,
    /// The terminal Rect where the page image was last rendered (for click mapping).
    pub image_area: Option<Rect>,
    /// Right page protocol for two-page spread view.
    pub protocol_right: Option<StatefulProtocol>,
    /// Right page image dimensions for spread view.
    pub image_size_right: Option<(u32, u32)>,
    /// Right page index currently rendered in spread view.
    pub rendered_page_right: Option<usize>,
}

impl PageViewState {
    pub fn new() -> Self {
        Self {
            protocol: None,
            rendered_page: None,
            image_size: None,
            text_lines: None,
            text_scroll: 0,
            image_area: None,
            protocol_right: None,
            image_size_right: None,
            rendered_page_right: None,
        }
    }

    /// Scroll text view down by `n` lines.
    pub fn scroll_down(&mut self, n: u16, visible_rows: u16) {
        if let Some((_, ref lines)) = self.text_lines {
            let max = (lines.len() as u16).saturating_sub(visible_rows);
            self.text_scroll = (self.text_scroll + n).min(max);
        }
    }

    /// Scroll text view up by `n` lines.
    pub fn scroll_up(&mut self, n: u16) {
        self.text_scroll = self.text_scroll.saturating_sub(n);
    }
}

/// Compute a centered sub-rect for an image with the given pixel dimensions,
/// fitted proportionally into the available terminal area.
/// `font_size` is (width, height) in pixels per cell.
fn centered_image_rect(img_w: u32, img_h: u32, area: Rect, font_size: (u16, u16)) -> Rect {
    if img_w == 0 || img_h == 0 || area.width == 0 || area.height == 0 {
        return area;
    }

    let (fw, fh) = (font_size.0.max(1) as f64, font_size.1.max(1) as f64);
    // Cell aspect ratio: cell_width_px / cell_height_px
    let cell_aspect = fw / fh;

    let aspect = img_w as f64 / img_h as f64;
    let area_w = area.width as f64;
    let area_h = area.height as f64;

    // Fitted dimensions in cells
    let (fit_w, fit_h) = {
        let w_from_h = area_h * aspect / cell_aspect;
        if w_from_h <= area_w {
            (w_from_h, area_h)
        } else {
            (area_w, area_w * cell_aspect / aspect)
        }
    };

    let fit_w = (fit_w.round() as u16).min(area.width);
    let fit_h = (fit_h.round() as u16).min(area.height);

    // Center horizontally
    let x_offset = (area.width.saturating_sub(fit_w)) / 2;

    Rect::new(
        area.x + x_offset,
        area.y,
        fit_w,
        fit_h,
    )
}

pub fn render(f: &mut Frame, area: Rect, app: &App, state: &mut PageViewState, font_size: (u16, u16)) {
    match app.view_mode {
        ViewMode::Image => {
            let bg_style = Style::default().bg(Color::Black);

            // Fill entire area with black background
            let bg_block = Block::default().borders(Borders::NONE).style(bg_style);
            f.render_widget(bg_block, area);

            // Two-page spread view
            if app.spread_mode != crate::app::SpreadMode::Off {
                let (left_page, right_page) = app.spread_pages();

                // Compute the width each page needs, then center the pair
                let left_size = state.image_size;
                let right_size = state.image_size_right;

                // Calculate how wide each page is in cells (using aspect ratio)
                let (fw, fh) = (font_size.0.max(1) as f64, font_size.1.max(1) as f64);
                let cell_aspect = fw / fh;

                let page_cell_width = |size: Option<(u32, u32)>| -> u16 {
                    if let Some((iw, ih)) = size {
                        let aspect = iw as f64 / ih as f64;
                        let w_from_h = area.height as f64 * aspect / cell_aspect;
                        (w_from_h.round() as u16).min(area.width / 2)
                    } else {
                        area.width / 2
                    }
                };

                let left_w = if left_page.is_some() { page_cell_width(left_size) } else { 0 };
                let right_w = if right_page.is_some() { page_cell_width(right_size) } else { 0 };
                let total_w = left_w + right_w;

                // Center the pair horizontally
                let x_offset = area.width.saturating_sub(total_w) / 2;
                let left_area = Rect::new(area.x + x_offset, area.y, left_w, area.height);
                let right_area = Rect::new(area.x + x_offset + left_w, area.y, right_w, area.height);

                // Render left page
                if left_page.is_some() {
                    if let Some(ref mut proto) = state.protocol {
                        let img_area = if let Some((iw, ih)) = state.image_size {
                            centered_image_rect(iw, ih, left_area, font_size)
                        } else {
                            left_area
                        };
                        state.image_area = Some(img_area);

                        let image = StatefulImage::new().resize(Resize::Scale(Some(ratatui_image::FilterType::Triangle)));
                        f.render_stateful_widget(image, img_area, proto);

                        if let Some(ref search) = app.search {
                            render_match_ticks(f, &search.current_page_match_positions, left_area, img_area);
                        }
                        if let Some(left_idx) = left_page {
                            if let Some(comments) = app.comments.get(left_idx) {
                                if !comments.is_empty() {
                                    let positions: Vec<f32> = comments.iter().map(|c| c.y_fraction).collect();
                                    render_comment_ticks(f, &positions, left_area, img_area);
                                }
                            }
                        }
                    }
                }

                // Render right page
                if right_page.is_some() {
                    if let Some(ref mut proto) = state.protocol_right {
                        let img_area = if let Some((iw, ih)) = state.image_size_right {
                            centered_image_rect(iw, ih, right_area, font_size)
                        } else {
                            right_area
                        };

                        let image = StatefulImage::new().resize(Resize::Scale(Some(ratatui_image::FilterType::Triangle)));
                        f.render_stateful_widget(image, img_area, proto);
                    }
                }

                if state.protocol.is_none() && state.protocol_right.is_none() {
                    let msg = if app.page_count() == 0 { "No pages loaded" } else { "Rendering..." };
                    let p = Paragraph::new(msg).style(bg_style);
                    f.render_widget(p, area);
                }
            } else if let Some(ref mut proto) = state.protocol {
                // Single page view (original code)
                // Center the image within the area
                let img_area = if let Some((iw, ih)) = state.image_size {
                    centered_image_rect(iw, ih, area, font_size)
                } else {
                    area
                };
                state.image_area = Some(img_area);

                let image = StatefulImage::new().resize(Resize::Scale(Some(ratatui_image::FilterType::Triangle)));
                f.render_stateful_widget(image, img_area, proto);

                // Render search match indicators in the left margin
                if let Some(ref search) = app.search {
                    render_match_ticks(f, &search.current_page_match_positions, area, img_area);
                }

                // Render comment indicators in the left margin (yellow)
                if let Some(comments) = app.comments.get(app.current_page()) {
                    if !comments.is_empty() {
                        let positions: Vec<f32> = comments.iter().map(|c| c.y_fraction).collect();
                        render_comment_ticks(f, &positions, area, img_area);
                    }
                }

                // Render form field indicators in the right margin (green)
                if app.mode == Mode::FormFilling {
                    let current_page = app.current_page();
                    let positions: Vec<(f32, bool)> = app.form_fields.iter()
                        .enumerate()
                        .filter(|(_, field)| field.page_num == current_page)
                        .map(|(i, field)| (field.y_fraction, i == app.form_field_index))
                        .collect();
                    render_form_field_ticks(f, &positions, area, img_area);
                }
            } else {
                let msg = if app.page_count() == 0 {
                    "No pages loaded"
                } else {
                    "Rendering..."
                };
                let p = Paragraph::new(msg).style(bg_style);
                f.render_widget(p, area);
            }
        }
        ViewMode::Text => {
            // Clear any leftover image protocol artifacts
            f.render_widget(Clear, area);

            let block = Block::default().borders(Borders::NONE);

            let query = app
                .search
                .as_ref()
                .map(|s| s.query.as_str())
                .unwrap_or("");

            let comment_fracs: Vec<f32> = app.comments.get(app.current_page())
                .map(|c| c.iter().map(|c| c.y_fraction).collect())
                .unwrap_or_default();

            let mut lines: Vec<Line> = if let Some((cached_page, ref text)) = state.text_lines {
                if cached_page == app.current_page() {
                    text.iter()
                        .map(|l| highlight_line(l, query))
                        .collect()
                } else {
                    vec![Line::raw("  Extracting text...")]
                }
            } else {
                vec![Line::raw("  Extracting text...")]
            };

            // Highlight lines near comment positions
            if !comment_fracs.is_empty() && lines.len() > 1 {
                let total = lines.len() as f32;
                for i in 0..lines.len() {
                    let line_frac = i as f32 / total;
                    if comment_fracs.iter().any(|&cf| (cf - line_frac).abs() < 1.5 / total) {
                        let line = std::mem::take(&mut lines[i]);
                        lines[i] = line.patch_style(theme::COMMENT_HIGHLIGHT);
                    }
                }
            }

            let p = Paragraph::new(lines)
                .block(block)
                .scroll((state.text_scroll, 0));
            f.render_widget(p, area);
        }
    }
}

/// Split a line into spans, highlighting case-insensitive matches of `query`.
///
/// Works correctly with multi-byte UTF-8 by mapping between byte offsets
/// of the original and lowercased strings via their char boundaries.
fn highlight_line<'a>(line: &'a str, query: &str) -> Line<'a> {
    if query.is_empty() {
        return Line::raw(line);
    }

    let lower = line.to_lowercase();
    let q_lower = query.to_lowercase();

    // Build a mapping from byte offsets in `lower` to byte offsets in `line`.
    // Both strings have the same number of chars, but may differ in byte lengths
    // (e.g. 'İ' (2 bytes) lowercases to 'i̇' (3 bytes)).
    let orig_offsets: Vec<usize> = line.char_indices().map(|(i, _)| i).collect();
    let lower_offsets: Vec<usize> = lower.char_indices().map(|(i, _)| i).collect();

    // Also need end-of-string offset
    let orig_end = line.len();
    let lower_end = lower.len();

    // Map a byte offset in `lower` to the corresponding byte offset in `line`.
    let lower_to_orig = |lower_byte: usize| -> usize {
        if lower_byte == lower_end {
            return orig_end;
        }
        // Find which char index this byte offset corresponds to
        match lower_offsets.binary_search(&lower_byte) {
            Ok(char_idx) => orig_offsets[char_idx],
            Err(_) => orig_end, // shouldn't happen at char boundaries
        }
    };

    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut last_end_lower = 0usize;

    let mut search_from = 0;
    while let Some(pos) = lower[search_from..].find(&q_lower) {
        let match_start_lower = search_from + pos;
        let match_end_lower = match_start_lower + q_lower.len();

        let match_start_orig = lower_to_orig(match_start_lower);
        let match_end_orig = lower_to_orig(match_end_lower);
        let last_end_orig = lower_to_orig(last_end_lower);

        // Add non-matching text before this match
        if match_start_orig > last_end_orig {
            spans.push(Span::raw(&line[last_end_orig..match_start_orig]));
        }

        // Add highlighted match
        spans.push(Span::styled(
            &line[match_start_orig..match_end_orig],
            theme::SEARCH_HIGHLIGHT,
        ));

        last_end_lower = match_end_lower;
        search_from = match_end_lower;
    }

    // Remaining text after last match
    let last_end_orig = lower_to_orig(last_end_lower);
    if last_end_orig < line.len() {
        spans.push(Span::raw(&line[last_end_orig..]));
    }

    if spans.is_empty() {
        Line::raw(line)
    } else {
        Line::from(spans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans_text(line: &Line) -> Vec<String> {
        line.spans.iter().map(|s| s.content.to_string()).collect()
    }

    #[test]
    fn highlight_no_query() {
        let line = highlight_line("hello world", "");
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "hello world");
    }

    #[test]
    fn highlight_no_match() {
        let line = highlight_line("hello world", "xyz");
        assert_eq!(line.spans.len(), 1);
    }

    #[test]
    fn highlight_single_match() {
        let line = highlight_line("hello world", "world");
        let texts = spans_text(&line);
        assert_eq!(texts, vec!["hello ", "world"]);
    }

    #[test]
    fn highlight_multiple_matches() {
        let line = highlight_line("abcabc", "abc");
        let texts = spans_text(&line);
        assert_eq!(texts, vec!["abc", "abc"]);
    }

    #[test]
    fn highlight_case_insensitive() {
        let line = highlight_line("Hello HELLO", "hello");
        let texts = spans_text(&line);
        assert_eq!(texts, vec!["Hello", " ", "HELLO"]);
    }

    #[test]
    fn highlight_ascii_safe() {
        // Basic ASCII search should always work
        let line = highlight_line("foo bar baz", "bar");
        let texts = spans_text(&line);
        assert_eq!(texts, vec!["foo ", "bar", " baz"]);
    }

    #[test]
    fn highlight_multibyte_utf8() {
        // This tests the UTF-8 safety of highlight_line.
        // With multi-byte chars, byte offsets from lowercase can differ.
        let line = highlight_line("café résumé", "é");
        // Should not panic and should find matches
        assert!(line.spans.len() > 1);
    }

    #[test]
    fn highlight_at_start() {
        let line = highlight_line("abc def", "abc");
        let texts = spans_text(&line);
        assert_eq!(texts, vec!["abc", " def"]);
    }

    #[test]
    fn centered_rect_wider_area() {
        let r = centered_image_rect(200, 400, Rect::new(0, 0, 80, 40), (8, 16));
        // Image is portrait, area is wide - should be centered horizontally
        assert!(r.x > 0);
        assert!(r.width < 80);
    }

    #[test]
    fn centered_rect_zero_image() {
        let area = Rect::new(5, 5, 40, 20);
        let r = centered_image_rect(0, 0, area, (8, 16));
        assert_eq!(r, area);
    }
}

/// Render red tick marks in the left margin at match positions.
fn render_match_ticks(
    f: &mut Frame,
    positions: &[f32],
    full_area: Rect,
    img_area: Rect,
) {
    if positions.is_empty() || img_area.height == 0 {
        return;
    }

    let tick_style = Style::default().fg(Color::Red).bg(Color::Red);

    // Use the left margin (between sidebar edge and image)
    let margin_x = if img_area.x > full_area.x + 1 {
        img_area.x - 1
    } else {
        full_area.x
    };

    for &frac in positions {
        let row = img_area.y + (frac * img_area.height as f32).round() as u16;
        if row >= full_area.y && row < full_area.y + full_area.height {
            let tick_area = Rect::new(margin_x, row, 1, 1);
            f.render_widget(Paragraph::new("▌").style(tick_style), tick_area);
        }
    }
}

/// Render green tick marks in the right margin at form field positions.
fn render_form_field_ticks(
    f: &mut Frame,
    positions: &[(f32, bool)], // (y_fraction, is_active)
    full_area: Rect,
    img_area: Rect,
) {
    if positions.is_empty() || img_area.height == 0 {
        return;
    }

    // Use the right margin (after comment ticks, offset by 1 more)
    let margin_x = (img_area.x + img_area.width + 1).min(full_area.x + full_area.width - 1);

    for &(frac, is_active) in positions {
        let tick_style = if is_active {
            theme::FORM_FIELD_ACTIVE
        } else {
            theme::FORM_FIELD
        };
        let bg_style = if is_active {
            Style::default().fg(Color::LightGreen).bg(Color::LightGreen)
        } else {
            Style::default().fg(Color::Green).bg(Color::Green)
        };
        let row = img_area.y + (frac * img_area.height as f32).round() as u16;
        if row >= full_area.y && row < full_area.y + full_area.height {
            let tick_area = Rect::new(margin_x, row, 1, 1);
            let _ = tick_style; // used for text-mode styling if needed
            f.render_widget(Paragraph::new("▐").style(bg_style), tick_area);
        }
    }
}

/// Render yellow tick marks in the right margin at comment positions.
fn render_comment_ticks(
    f: &mut Frame,
    positions: &[f32],
    full_area: Rect,
    img_area: Rect,
) {
    if positions.is_empty() || img_area.height == 0 {
        return;
    }

    let tick_style = Style::default().fg(Color::Yellow).bg(Color::Yellow);

    // Use the right margin (after the image)
    let margin_x = (img_area.x + img_area.width).min(full_area.x + full_area.width - 1);

    for &frac in positions {
        let row = img_area.y + (frac * img_area.height as f32).round() as u16;
        if row >= full_area.y && row < full_area.y + full_area.height {
            let tick_area = Rect::new(margin_x, row, 1, 1);
            f.render_widget(Paragraph::new("▐").style(tick_style), tick_area);
        }
    }
}

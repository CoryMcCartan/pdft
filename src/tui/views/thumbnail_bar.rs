use image::DynamicImage;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders};
use ratatui_image::{Resize, StatefulImage};
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;

use crate::app::App;
use crate::tui::theme;

/// Fixed thumbnail cell height in terminal rows.
const THUMB_CELL_HEIGHT: u16 = 8;

/// State for the thumbnail strip.
pub struct ThumbnailBarState {
    /// Cached rendered images, indexed by page number.
    pub images: Vec<Option<DynamicImage>>,
    /// Cached display protocols, indexed by page number.
    /// Created lazily from images and reused across frames.
    protocols: Vec<Option<StatefulProtocol>>,
}

impl ThumbnailBarState {
    pub fn new() -> Self {
        Self {
            images: Vec::new(),
            protocols: Vec::new(),
        }
    }

    pub fn ensure_capacity(&mut self, page_count: usize) {
        if self.images.len() < page_count {
            self.images.resize_with(page_count, || None);
        }
        if self.protocols.len() < page_count {
            self.protocols.resize_with(page_count, || None);
        }
    }

    pub fn clear(&mut self) {
        self.images.clear();
        self.protocols.clear();
    }

    /// Get or create a protocol for the given page index.
    /// Returns None if no image is cached for this page.
    fn get_protocol(&mut self, page_idx: usize, picker: &Picker) -> Option<&mut StatefulProtocol> {
        if page_idx >= self.images.len() {
            return None;
        }
        // Create protocol from image if needed
        if self.protocols[page_idx].is_none() {
            if let Some(img) = &self.images[page_idx] {
                self.protocols[page_idx] = Some(picker.new_resize_protocol(img.clone()));
            }
        }
        self.protocols[page_idx].as_mut()
    }
}

/// Get the group label for a page, if assigned.
fn group_label<'a>(app: &'a App, page_idx: usize) -> Option<&'a str> {
    let slot = app.workspace.pages.get(page_idx)?;
    let id = slot.output_target?;
    Some(app.workspace.output_targets.get(id)?.label.as_str())
}

/// Compute thumbnail cell width from a fixed cell height.
fn thumb_cell_width(cell_height: u16) -> u16 {
    ((cell_height as f32) * 1.4).ceil() as u16
}

/// Render a single thumbnail cell at the given area.
fn render_thumb(
    f: &mut Frame,
    area: Rect,
    page_idx: usize,
    is_current: bool,
    is_deleted: bool,
    group_label: Option<&str>,
    proto: Option<&mut StatefulProtocol>,
) {
    let border_style = if is_current {
        Style::default().fg(Color::Cyan)
    } else if is_deleted {
        Style::default().fg(Color::Red)
    } else {
        theme::BORDER
    };

    let mut title = format!(" {} ", page_idx + 1);
    if is_deleted {
        title.push('×');
    }
    if let Some(lbl) = group_label {
        title.push_str(lbl);
    }
    if is_deleted || group_label.is_some() {
        title.push(' ');
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title);

    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(proto) = proto {
        let image = StatefulImage::new().resize(Resize::Scale(Some(ratatui_image::FilterType::Triangle)));
        f.render_stateful_widget(image, inner, proto);
    }
}

/// Render as a single horizontal strip (normal mode).
pub fn render(f: &mut Frame, area: Rect, app: &App, state: &mut ThumbnailBarState, picker: &Picker) {
    if area.height < 3 || area.width < 10 {
        return;
    }

    let current = app.current_page();
    let page_count = app.page_count();
    if page_count == 0 {
        return;
    }

    state.ensure_capacity(page_count);

    let thumb_w = thumb_cell_width(area.height);
    let cols = (area.width / (thumb_w + 1)).max(1) as usize;

    let half = cols / 2;
    let start = if current >= half {
        (current - half).min(page_count.saturating_sub(cols))
    } else {
        0
    };
    let end = (start + cols).min(page_count);

    let mut x = area.x;
    for page_idx in start..end {
        let w = thumb_w.min(area.width.saturating_sub(x - area.x));
        if w < 4 {
            break;
        }
        let thumb_area = Rect::new(x, area.y, w, area.height);

        let is_current = page_idx == current;
        let is_deleted = app.workspace.pages.get(page_idx).is_some_and(|p| p.marked_for_delete);
        let label = group_label(app, page_idx);
        let proto = state.get_protocol(page_idx, picker);

        render_thumb(f, thumb_area, page_idx, is_current, is_deleted, label, proto);

        x += w + 1;
    }
}

/// Render as a wrapping grid filling the whole area (thumbnails-only mode).
pub fn render_grid(f: &mut Frame, area: Rect, app: &App, state: &mut ThumbnailBarState, picker: &Picker) {
    if area.height < 3 || area.width < 10 {
        return;
    }

    let current = app.current_page();
    let page_count = app.page_count();
    if page_count == 0 {
        return;
    }

    state.ensure_capacity(page_count);

    let cell_h = THUMB_CELL_HEIGHT;
    let cell_w = thumb_cell_width(cell_h);
    let cols = (area.width / (cell_w + 1)).max(1) as usize;
    let rows = (area.height / cell_h).max(1) as usize;
    let per_page = cols * rows;

    let screen_start = {
        let screen_of_current = current / per_page;
        screen_of_current * per_page
    };
    let screen_end = (screen_start + per_page).min(page_count);

    for (i, page_idx) in (screen_start..screen_end).enumerate() {
        let col = i % cols;
        let row = i / cols;
        let x = area.x + (col as u16) * (cell_w + 1);
        let y = area.y + (row as u16) * cell_h;

        if x + cell_w > area.x + area.width || y + cell_h > area.y + area.height {
            break;
        }

        let thumb_area = Rect::new(x, y, cell_w, cell_h);
        let is_current = page_idx == current;
        let is_deleted = app.workspace.pages.get(page_idx).is_some_and(|p| p.marked_for_delete);
        let label = group_label(app, page_idx);
        let proto = state.get_protocol(page_idx, picker);

        render_thumb(f, thumb_area, page_idx, is_current, is_deleted, label, proto);
    }
}

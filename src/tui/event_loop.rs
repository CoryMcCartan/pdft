use crate::app::{App, LayoutMode, TextScroll, ViewMode};
use crate::render::cache::ImageCache;
use crate::render::renderer;
use crate::render::text_layout;
use crate::tui::input;
use crate::tui::views::{dialog, dialog::InputDialog, help, page_view, sidebar, status_bar, thumbnail_bar};
use anyhow::Result;
use crossterm::event::{self, Event};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::execute;
use hayro::RenderCache;
use hayro::hayro_syntax::Pdf;
use page_view::PageViewState;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui_image::picker::{Picker, ProtocolType};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use thumbnail_bar::ThumbnailBarState;

/// Holds hayro PDFs for rendering.
struct PdfStore {
    pdfs: Vec<Pdf>,
}

impl PdfStore {
    fn new() -> Self {
        Self { pdfs: Vec::new() }
    }

    /// Load from raw bytes (avoids re-reading the file).
    fn load_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let pdf = Pdf::new(bytes.to_vec())
            .map_err(|e| anyhow::anyhow!("failed to parse PDF: {e:?}"))?;
        self.pdfs.push(pdf);
        Ok(())
    }

    /// Fallback: load from path (for merged docs without cached bytes).
    fn load(&mut self, path: &Path) -> Result<()> {
        let bytes = std::fs::read(path)?;
        let pdf = Pdf::new(bytes)
            .map_err(|e| anyhow::anyhow!("failed to parse PDF: {e:?}"))?;
        self.pdfs.push(pdf);
        Ok(())
    }

    fn get(&self, doc_id: usize) -> Option<&Pdf> {
        self.pdfs.get(doc_id)
    }
}

/// Max thumbnails to render per event loop tick to stay responsive.
const THUMB_BATCH_SIZE: usize = 3;

/// Start a background file watcher that sends the path on the channel when it changes.
fn start_file_watcher(path: PathBuf, tx: mpsc::Sender<PathBuf>) -> Result<()> {
    use notify_debouncer_mini::{new_debouncer, notify};
    use std::time::Duration as Dur;

    let watch_path = path.clone();
    let mut debouncer = new_debouncer(Dur::from_millis(500), move |res: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
        if res.is_ok() {
            let _ = tx.send(path.clone());
        }
    }).map_err(|e| anyhow::anyhow!("failed to create file watcher: {e}"))?;

    debouncer.watcher().watch(&watch_path, notify::RecursiveMode::NonRecursive)
        .map_err(|e| anyhow::anyhow!("failed to watch file: {e}"))?;

    // Leak the debouncer so it lives for the duration of the process
    std::mem::forget(debouncer);
    Ok(())
}

/// Reload the PDF from disk, replacing the document and re-rendering.
fn reload_pdf(
    path: &Path,
    app: &mut App,
    pdf_store: &mut PdfStore,
    page_state: &mut PageViewState,
    thumb_state: &mut ThumbnailBarState,
    image_cache: &mut ImageCache,
) {
    // Re-open the document (PdfDocument::open reads the file and caches raw bytes).
    let mut doc = match crate::model::document::PdfDocument::open(path) {
        Ok(d) => d,
        Err(e) => {
            app.status_message = Some(format!("Watch: {e}"));
            return;
        }
    };

    let page_count = doc.page_count();

    // Reload hayro PDF using the bytes already held by the new doc, then drop them.
    pdf_store.pdfs.clear();
    let load_result = match doc.raw_bytes() {
        Some(bytes) => pdf_store.load_bytes(bytes),
        None => pdf_store.load(path),
    };
    doc.drop_raw_bytes();
    if let Err(e) = load_result {
        app.status_message = Some(format!("Watch render: {e}"));
        return;
    }

    app.workspace.documents[0] = doc;

    // Rebuild page list
    app.workspace.pages.clear();
    for page_num in 0..page_count {
        app.workspace.pages.push(crate::model::page_ref::PageSlot {
            source: crate::model::page_ref::PageRef { doc_id: 0, page_num },
            output_target: None,
            marked_for_delete: false,
        });
    }

    // Clamp selected page
    if app.workspace.selected_page >= page_count {
        app.workspace.selected_page = page_count.saturating_sub(1);
    }

    // Invalidate all caches
    image_cache.clear();
    page_state.rendered_page = None;
    page_state.rendered_page_right = None;
    page_state.text_lines = None;
    thumb_state.clear();

    // Re-extract comments
    app.extract_comments();

    app.status_message = Some("Reloaded".into());
}

/// Launch the TUI for viewing a PDF file.
pub fn run(path: &Path, force_halfblock: bool, start_text: bool, start_page: Option<usize>, watch: bool) -> Result<()> {
    let mut picker = Picker::from_query_stdio()
        .unwrap_or_else(|_| Picker::from_fontsize((8, 16)));
    if force_halfblock {
        picker.set_protocol_type(ProtocolType::Halfblocks);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    execute!(stdout, crossterm::event::EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    if start_text || picker.protocol_type() == ProtocolType::Halfblocks {
        app.view_mode = crate::app::ViewMode::Text;
    }
    app.open_file(path)?;
    if let Some(p) = start_page {
        let idx = p.saturating_sub(1).min(app.page_count().saturating_sub(1));
        app.workspace.selected_page = idx;
    }
    app.extract_comments();

    let mut pdf_store = PdfStore::new();
    // Use the raw bytes already loaded by PdfDocument to avoid re-reading the file,
    // then drop them — the hayro PdfStore now owns the only copy.
    match app.workspace.documents[0].raw_bytes() {
        Some(bytes) => pdf_store.load_bytes(bytes)?,
        None => pdf_store.load(path)?,
    }
    app.workspace.documents[0].drop_raw_bytes();

    let mut page_state = PageViewState::new();
    let mut thumb_state = ThumbnailBarState::new();
    let mut input_dialog = InputDialog::new("", "");
    let mut image_cache = ImageCache::new(16);

    let font_size = picker.font_size();
    let sz = terminal.size()?;
    let term_rect = Rect::new(0, 0, sz.width, sz.height);

    render_current_page(&pdf_store, &app, &picker, &mut page_state, &mut image_cache, term_rect, font_size);
    render_spread_page(&pdf_store, &app, &picker, &mut page_state, &mut image_cache, term_rect, font_size);
    if app.view_mode == crate::app::ViewMode::Text {
        extract_current_text(&pdf_store, &app, &mut page_state, term_rect);
    }
    render_nearby_thumbnails(&pdf_store, &app, &mut thumb_state, font_size, term_rect, THUMB_BATCH_SIZE);

    // Set up file watcher if --watch was passed
    let watch_rx = if watch {
        let (tx, rx) = mpsc::channel();
        let watch_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        app.watching = true;
        start_file_watcher(watch_path, tx)?;
        Some(rx)
    } else {
        None
    };

    let result = run_loop(
        &mut terminal,
        &mut app,
        &mut pdf_store,
        &picker,
        &mut page_state,
        &mut thumb_state,
        &mut input_dialog,
        &mut image_cache,
        watch_rx.as_ref(),
    );

    // Drop all protocol/image state BEFORE restoring the terminal.
    // Protocol destructors may write escape sequences; they must go
    // to the alternate screen, not the user's shell.
    drop(page_state);
    drop(thumb_state);
    drop(image_cache);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::event::DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    pdf_store: &mut PdfStore,
    picker: &Picker,
    page_state: &mut PageViewState,
    thumb_state: &mut ThumbnailBarState,
    input_dialog: &mut InputDialog,
    image_cache: &mut ImageCache,
    watch_rx: Option<&mpsc::Receiver<PathBuf>>,
) -> Result<()> {
    let font_size = picker.font_size();

    loop {
        let sz = terminal.size()?;
        let term_size = Rect::new(0, 0, sz.width, sz.height);

        // Check for file change notifications
        if let Some(rx) = watch_rx {
            if let Ok(path) = rx.try_recv() {
                // Drain any additional queued notifications
                while rx.try_recv().is_ok() {}
                reload_pdf(&path, app, pdf_store, page_state, thumb_state, image_cache);
                if app.layout_mode != LayoutMode::ThumbnailsOnly {
                    render_current_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                    render_spread_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                }
                if app.view_mode == ViewMode::Text {
                    extract_current_text(pdf_store, app, page_state, term_size);
                }
            }
        }

        terminal.draw(|f| {
            if app.fullscreen {
                page_view::render(f, f.area(), app, page_state, font_size);
                if app.show_help {
                    help::render(f, f.area());
                }
                return;
            }

            let show_main = app.layout_mode != LayoutMode::ThumbnailsOnly;
            let show_thumbs = app.layout_mode != LayoutMode::NoThumbnails;

            let mut constraints = vec![Constraint::Length(1)]; // status bar
            if show_main {
                constraints.push(Constraint::Min(5)); // main area
            }
            if show_thumbs {
                constraints.push(if show_main {
                    Constraint::Length(8)
                } else {
                    Constraint::Min(5)
                });
            }
            constraints.push(Constraint::Length(1)); // command hints

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(f.area());

            let mut chunk_idx = 0;

            // Status bar
            status_bar::render(f, chunks[chunk_idx], app);
            chunk_idx += 1;

            // Main area (with sidebar)
            if show_main {
                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(12),
                        Constraint::Min(20),
                    ])
                    .split(chunks[chunk_idx]);

                sidebar::render(f, main_chunks[0], app);
                page_view::render(f, main_chunks[1], app, page_state, font_size);
                chunk_idx += 1;
            }

            // Thumbnail strip or grid (with sidebar in grid mode)
            if show_thumbs {
                if show_main {
                    thumbnail_bar::render(f, chunks[chunk_idx], app, thumb_state, picker);
                } else {
                    // Grid mode: sidebar + thumbnail grid
                    let grid_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Length(12),
                            Constraint::Min(20),
                        ])
                        .split(chunks[chunk_idx]);

                    sidebar::render(f, grid_chunks[0], app);
                    thumbnail_bar::render_grid(f, grid_chunks[1], app, thumb_state, picker);
                }
                chunk_idx += 1;
            }

            // Bottom bar: dialog, pending assign prompt, or command hints
            if input_dialog.active {
                dialog::render(f, chunks[chunk_idx], input_dialog);
            } else if app.mode == crate::app::Mode::TextPlacing {
                let line = ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(" ADD TEXT ", ratatui::style::Style::default()
                        .fg(ratatui::style::Color::Black)
                        .bg(ratatui::style::Color::Green)),
                    ratatui::text::Span::raw(" click to place  "),
                    ratatui::text::Span::styled(":", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":coords  "),
                    ratatui::text::Span::styled("Esc", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":cancel"),
                ]);
                f.render_widget(ratatui::widgets::Paragraph::new(line), chunks[chunk_idx]);
            } else if app.mode == crate::app::Mode::FormFilling {
                let line = ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(" FORM ", ratatui::style::Style::default()
                        .fg(ratatui::style::Color::Black)
                        .bg(ratatui::style::Color::Green)),
                    ratatui::text::Span::raw("  "),
                    ratatui::text::Span::styled("Tab", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":next  "),
                    ratatui::text::Span::styled("S-Tab", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":prev  "),
                    ratatui::text::Span::styled("Enter", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":edit  "),
                    ratatui::text::Span::raw("click:select  "),
                    ratatui::text::Span::styled("Esc", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":done"),
                ]);
                f.render_widget(ratatui::widgets::Paragraph::new(line), chunks[chunk_idx]);
            } else if app.mode == crate::app::Mode::SignaturePlacing {
                let line = ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(" SIGNATURE ", ratatui::style::Style::default()
                        .fg(ratatui::style::Color::Black)
                        .bg(ratatui::style::Color::Yellow)),
                    ratatui::text::Span::raw(" click to place  "),
                    ratatui::text::Span::styled(":", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":coords  "),
                    ratatui::text::Span::styled("Esc", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":cancel"),
                ]);
                f.render_widget(ratatui::widgets::Paragraph::new(line), chunks[chunk_idx]);
            } else if app.pending_assign {
                let line = ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(" Assign to group: ", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw("press a-z"),
                ]);
                f.render_widget(ratatui::widgets::Paragraph::new(line), chunks[chunk_idx]);
            } else if app.visual_anchor.is_some() {
                let (a, b) = app.selected_range();
                let count = b - a + 1;
                let line = ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(" VISUAL ", ratatui::style::Style::default()
                        .fg(ratatui::style::Color::Black)
                        .bg(ratatui::style::Color::Cyan)),
                    ratatui::text::Span::raw(format!(" {count} page(s) selected  ")),
                    ratatui::text::Span::styled("d", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":del  "),
                    ratatui::text::Span::styled("a+key", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":group  "),
                    ratatui::text::Span::styled("Esc", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":cancel"),
                ]);
                f.render_widget(ratatui::widgets::Paragraph::new(line), chunks[chunk_idx]);
            } else {
                let hints = ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(" j/k", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":nav  "),
                    ratatui::text::Span::styled("v", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":select  "),
                    ratatui::text::Span::styled("d", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":del  "),
                    ratatui::text::Span::styled("a", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":assign  "),
                    ratatui::text::Span::styled("s", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":save  "),
                    ratatui::text::Span::styled("/", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":search  "),
                    ratatui::text::Span::styled("?", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":help  "),
                    ratatui::text::Span::styled("q", crate::tui::theme::HELP_KEY),
                    ratatui::text::Span::raw(":quit"),
                ]);
                f.render_widget(ratatui::widgets::Paragraph::new(hints), chunks[chunk_idx]);
            }

            if app.show_help {
                help::render(f, f.area());
            }
        })?;

        if app.should_quit {
            break;
        }

        // Incrementally render thumbnails that are still missing.
        // This runs every tick even without key events, progressively filling in.
        let rendered_any = if app.layout_mode != LayoutMode::NoThumbnails {
            render_nearby_thumbnails(
                pdf_store, app, thumb_state, font_size, term_size, THUMB_BATCH_SIZE,
            )
        } else {
            false
        };

        // Pre-render the next page into the image cache so navigation is instant.
        // Only runs if there's nothing more urgent (thumbnails still loading).
        let prefetched = if !rendered_any && app.layout_mode != LayoutMode::ThumbnailsOnly {
            prefetch_next_page(pdf_store, app, image_cache, term_size, font_size)
        } else {
            false
        };

        // Use shorter poll when background work is still running so we redraw quickly
        let poll_ms = if rendered_any || prefetched { 10 } else { 100 };

        if event::poll(Duration::from_millis(poll_ms))? {
            // Read the event and drain any queued duplicates (debounce).
            // This prevents key-repeat from queueing up many page turns
            // that continue after the key is released.
            let mut key_event = None;
            let mut mouse_event = None;
            let mut resized = false;
            match event::read()? {
                Event::Key(key) => {
                    key_event = Some(key);
                    // Drain queued events, keeping only the last key event
                    while event::poll(Duration::from_millis(0))? {
                        match event::read()? {
                            Event::Key(next_key) => key_event = Some(next_key),
                            Event::Resize(_, _) => resized = true,
                            _ => {}
                        }
                    }
                }
                Event::Mouse(mouse) => mouse_event = Some(mouse),
                Event::Resize(_, _) => resized = true,
                _ => {}
            }

            // On resize, invalidate rendered state so it redraws at the new size
            if resized {
                page_state.rendered_page = None;
                page_state.rendered_page_right = None;
                page_state.text_lines = None;
                thumb_state.clear();
                image_cache.clear();
                let _ = terminal.clear();
                render_current_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                render_spread_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                if app.view_mode == ViewMode::Text {
                    extract_current_text(pdf_store, app, page_state, term_size);
                }
            }

            // Handle mouse events
            if let Some(mouse) = mouse_event {
                let prev_page = app.current_page();
                let img_area = page_state.image_area;
                let page_dims = app.workspace.pages.get(app.current_page())
                    .and_then(|slot| {
                        let doc = &app.workspace.documents[slot.source.doc_id];
                        doc.page_dimensions().get(slot.source.page_num).copied()
                    });
                let consumed = input::handle_mouse(app, mouse, term_size, img_area, page_dims);
                if consumed && app.current_page() != prev_page {
                    page_state.text_scroll = 0;
                    page_state.rendered_page = None;
                    page_state.rendered_page_right = None;
                    if app.layout_mode != LayoutMode::ThumbnailsOnly {
                        render_current_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                        render_spread_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                    }
                    if app.view_mode == ViewMode::Text && app.layout_mode != LayoutMode::ThumbnailsOnly {
                        extract_current_text(pdf_store, app, page_state, term_size);
                    }
                    update_match_positions(pdf_store, app);
                }
                // Open text input dialog after click-to-place sets TextContentInput mode
                if app.mode == crate::app::Mode::TextContentInput && !input_dialog.active {
                    input_dialog.title = "Add text".into();
                    input_dialog.prompt = "Text:".into();
                    input_dialog.open();
                }
                // Open form field dialog after click selects a field
                if app.mode == crate::app::Mode::FormFieldInput && !input_dialog.active {
                    if app.form_field_index < app.form_fields.len() {
                        input::open_form_field_dialog(app, input_dialog);
                    }
                }
            }

            if let Some(key) = key_event {
                let prev_page = app.current_page();
                let prev_page_count = app.page_count();
                let prev_doc_count = app.workspace.documents.len();
                let prev_history_len = app.workspace.history.len();
                let prev_layout = app.layout_mode;
                let prev_view_mode = app.view_mode;
                let prev_spread = app.spread_mode;
                let prev_fullscreen = app.fullscreen;
                let had_help = app.show_help;
                let had_dialog = input_dialog.active;

                input::handle_key(app, key, input_dialog);

                // New document merged
                if app.workspace.documents.len() > prev_doc_count {
                    for i in prev_doc_count..app.workspace.documents.len() {
                        let load_result = match app.workspace.documents[i].raw_bytes() {
                            Some(bytes) => pdf_store.load_bytes(bytes),
                            None => {
                                let path = app.workspace.documents[i].path.clone();
                                pdf_store.load(&path)
                            }
                        };
                        if let Err(e) = load_result {
                            app.status_message = Some(format!("Render load error: {e}"));
                        }
                    }
                    thumb_state.clear();
                    image_cache.clear();
                    app.extract_comments();
                }

                // Execute pending search
                let needs_search = app
                    .search
                    .as_ref()
                    .is_some_and(|s| s.matches.is_empty() && !s.query.is_empty());
                if needs_search {
                    // Show searching status before the blocking operation
                    app.status_message = Some("Searching...".into());
                    terminal.draw(|f| {
                        status_bar::render(f, Rect::new(0, 0, f.area().width, 1), app);
                    })?;

                    let page_count = app.page_count();
                    let mut search = app.search.take().unwrap();
                    execute_search(pdf_store, page_count, &app.workspace, &mut search);
                    if search.matches.is_empty() {
                        app.status_message = Some(format!("No matches for '{}'", search.query));
                    } else {
                        let cur = app.current_page();
                        let idx = search
                            .matches
                            .iter()
                            .position(|(p, _)| *p >= cur)
                            .unwrap_or(0);
                        search.current_match = idx;
                        let (page_idx, _) = search.matches[idx];
                        app.workspace.selected_page = page_idx;
                        app.scroll_to_match = true;
                        let total = search.matches.len();
                        app.status_message =
                            Some(format!("Match {}/{total} for '{}'", idx + 1, search.query));
                    }
                    app.search = Some(search);
                }

                let layout_changed = app.layout_mode != prev_layout;
                let overlay_dismissed = (had_help && !app.show_help)
                    || (had_dialog && !input_dialog.active);

                // Full terminal clear for graphics protocol artifacts
                if layout_changed || overlay_dismissed {
                    page_state.rendered_page = None;
                    let _ = terminal.clear();
                }

                // Page count changed (e.g. undo of merge) — refresh thumbnails
                let page_count_changed = app.page_count() != prev_page_count;
                if page_count_changed {
                    thumb_state.clear();
                    image_cache.clear();
                }

                let state_changed = app.workspace.history.len() != prev_history_len;

                let view_mode_changed = app.view_mode != prev_view_mode;
                let spread_changed = app.spread_mode != prev_spread;
                let fullscreen_changed = app.fullscreen != prev_fullscreen;

                // Spread or fullscreen toggle requires full clear and cache invalidation (resolution changes)
                if spread_changed || fullscreen_changed {
                    page_state.rendered_page = None;
                    page_state.rendered_page_right = None;
                    image_cache.clear();
                    let _ = terminal.clear();
                }

                let needs_rerender = app.current_page() != prev_page
                    || app.workspace.documents.len() > prev_doc_count
                    || layout_changed
                    || overlay_dismissed
                    || page_count_changed
                    || state_changed
                    || view_mode_changed
                    || spread_changed
                    || fullscreen_changed;

                if needs_rerender {
                    // Reset text scroll when page changes
                    if app.current_page() != prev_page {
                        page_state.text_scroll = 0;
                    }

                    // Only render the main page if it's actually visible
                    if app.layout_mode != LayoutMode::ThumbnailsOnly {
                        page_state.rendered_page = None;
                        page_state.rendered_page_right = None;
                        render_current_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                        render_spread_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                    }

                    // Extract text if in text mode and page changed
                    if app.view_mode == ViewMode::Text
                        && app.layout_mode != LayoutMode::ThumbnailsOnly
                    {
                        extract_current_text(pdf_store, app, page_state, term_size);
                    }
                    // Only render thumbnails if visible and some are missing
                    if app.layout_mode != LayoutMode::NoThumbnails {
                        render_nearby_thumbnails(pdf_store, app, thumb_state, font_size, term_size, THUMB_BATCH_SIZE);
                    }

                    // Update search match positions for current page
                    if app.current_page() != prev_page || needs_search {
                        update_match_positions(pdf_store, app);
                    }

                    // Scroll text view to match position after search navigation
                    if app.scroll_to_match && app.view_mode == ViewMode::Text {
                        if let Some(ref search) = app.search {
                            if let Some(&first_frac) = search.current_page_match_positions.first() {
                                if let Some((_, ref lines)) = page_state.text_lines {
                                    let total_lines = lines.len() as u16;
                                    let visible_rows = term_size.height.saturating_sub(12);
                                    let target_line = (first_frac * total_lines as f32) as u16;
                                    // Center the match in the viewport
                                    page_state.text_scroll = target_line.saturating_sub(visible_rows / 2);
                                    let max = total_lines.saturating_sub(visible_rows);
                                    page_state.text_scroll = page_state.text_scroll.min(max);
                                }
                            }
                        }
                    }
                    app.scroll_to_match = false;
                }

            }

            // Refresh after document mutation (text stamp, form field edit, etc.)
            if app.needs_pdf_refresh {
                app.needs_pdf_refresh = false;
                let doc_id = app.workspace.pages.get(app.current_page())
                    .map(|s| s.source.doc_id).unwrap_or(0);
                let doc = &mut app.workspace.documents[doc_id];
                match doc.refresh_bytes() {
                    Err(e) => {
                        app.status_message = Some(format!("Refresh error: {e}"));
                    }
                    Ok(bytes) => {
                        match hayro::hayro_syntax::Pdf::new(bytes) {
                            Ok(pdf) => {
                                if doc_id < pdf_store.pdfs.len() {
                                    pdf_store.pdfs[doc_id] = pdf;
                                }
                                // Invalidate current page cache and re-render
                                page_state.rendered_page = None;
                                page_state.rendered_page_right = None;
                                page_state.text_lines = None;
                                image_cache.clear();
                                render_current_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                                render_spread_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                                if app.view_mode == ViewMode::Text {
                                    extract_current_text(pdf_store, app, page_state, term_size);
                                }
                            }
                            Err(e) => {
                                app.status_message = Some(format!("Render refresh failed: {e:?}"));
                            }
                        }
                    }
                }
            }

            // Execute pending signature placement
            if let Some((pdf_x, pdf_y)) = app.pending_signature.take() {
                if let Some(sig_path) = app.signature_path.clone() {
                    let page_idx = app.current_page();
                    let slot = &app.workspace.pages[page_idx];
                    let doc_id = slot.source.doc_id;
                    let page_num = slot.source.page_num;
                    let sig_width = app.signature_width_pt;

                    let doc = &mut app.workspace.documents[doc_id];
                    match doc.embed_signature(page_num, &sig_path, pdf_x, pdf_y, sig_width) {
                        Ok(()) => {
                            // Refresh bytes for hayro and reload
                            match doc.refresh_bytes() {
                                Err(e) => {
                                    app.status_message = Some(format!("Refresh error: {e}"));
                                }
                                Ok(bytes) => {
                                    match hayro::hayro_syntax::Pdf::new(bytes) {
                                        Ok(pdf) => {
                                            if doc_id < pdf_store.pdfs.len() {
                                                pdf_store.pdfs[doc_id] = pdf;
                                            }
                                        }
                                        Err(e) => {
                                            app.status_message = Some(format!("PDF reload error: {e:?}"));
                                        }
                                    }
                                    // Invalidate caches
                                    page_state.rendered_page = None;
                                    page_state.rendered_page_right = None;
                                    image_cache.clear();
                                    thumb_state.clear();
                                    render_current_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                                    render_spread_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                                    app.status_message = Some("Signature placed. Save to keep changes.".into());
                                }
                            }
                        }
                        Err(e) => {
                            app.status_message = Some(format!("Signature error: {e}"));
                        }
                    }
                }
            }

            // Undo last document-level mutation (signature or text stamp)
            if app.pending_signature_undo {
                app.pending_signature_undo = false;
                // Find which document has something to undo (prefer text stamps as most recent)
                let doc_id = app.workspace.documents.iter()
                    .rposition(|d| !d.text_stamp_undos.is_empty() || !d.signature_undos.is_empty());
                if let Some(doc_id) = doc_id {
                    let doc = &mut app.workspace.documents[doc_id];
                    // Try text stamp undo first, then signature undo
                    let undone = if !doc.text_stamp_undos.is_empty() {
                        doc.undo_text_stamp()
                    } else {
                        doc.undo_signature()
                    };
                    if undone {
                        match doc.refresh_bytes() {
                            Err(e) => {
                                app.status_message = Some(format!("Undo refresh error: {e}"));
                            }
                            Ok(bytes) => {
                                match hayro::hayro_syntax::Pdf::new(bytes) {
                                    Ok(pdf) => {
                                        if doc_id < pdf_store.pdfs.len() {
                                            pdf_store.pdfs[doc_id] = pdf;
                                        }
                                    }
                                    Err(e) => {
                                        app.status_message = Some(format!("Undo reload error: {e:?}"));
                                    }
                                }
                                page_state.rendered_page = None;
                                page_state.rendered_page_right = None;
                                image_cache.clear();
                                thumb_state.clear();
                                render_current_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                                render_spread_page(pdf_store, app, picker, page_state, image_cache, term_size, font_size);
                                app.status_message = Some("Undone".into());
                            }
                        }
                    }
                }
            }

            // Apply text scroll command (from keyboard or mouse)
            let scroll = app.text_scroll;
            if scroll != TextScroll::None {
                app.text_scroll = TextScroll::None;
                let visible_rows = term_size.height.saturating_sub(12);
                match scroll {
                    TextScroll::Bottom => page_state.scroll_down(u16::MAX / 2, visible_rows),
                    TextScroll::Top => page_state.text_scroll = 0,
                    TextScroll::Lines(n) if n > 0 => page_state.scroll_down(n as u16, visible_rows),
                    TextScroll::Lines(n) => page_state.scroll_up((-n) as u16),
                    TextScroll::None => {}
                }
            }
        }
    }

    Ok(())
}

/// Pre-render the next page into the image cache so navigation to it is instant.
/// Uses the same resolution parameters as render_current_page.
fn prefetch_next_page(
    store: &PdfStore,
    app: &App,
    image_cache: &mut ImageCache,
    term_size: Rect,
    font_size: (u16, u16),
) -> bool {
    let current = app.current_page();
    let page_count = app.page_count();
    let next_idx = current + 1;
    if next_idx >= page_count {
        return false;
    }

    let slot = match app.workspace.pages.get(next_idx) {
        Some(s) => s,
        None => return false,
    };
    let doc_id = slot.source.doc_id;
    let page_num = slot.source.page_num;

    let base_cols = if app.fullscreen {
        term_size.width
    } else {
        term_size.width.saturating_sub(14)
    };
    let view_cols = if app.spread_mode != crate::app::SpreadMode::Off { base_cols / 2 } else { base_cols };
    let view_rows = if app.fullscreen {
        term_size.height
    } else {
        match app.layout_mode {
            LayoutMode::NoThumbnails => term_size.height.saturating_sub(3),
            _ => term_size.height.saturating_sub(11),
        }
    };
    let (max_w, max_h) = area_to_pixels(Rect::new(0, 0, view_cols, view_rows), font_size);

    if image_cache.get(doc_id, page_num, max_w, max_h).is_some() {
        return false; // already cached
    }

    let pdf = match store.get(doc_id) {
        Some(p) => p,
        None => return false,
    };

    let cache = RenderCache::new();
    if let Ok(image) = renderer::render_page_with_cache(pdf, page_num, 2.0, Some(max_w), Some(max_h), &cache) {
        image_cache.put(doc_id, page_num, max_w, max_h, image);
        true
    } else {
        false
    }
}

/// Compute pixel dimensions from a terminal cell area and font size.
fn area_to_pixels(area: Rect, font_size: (u16, u16)) -> (u16, u16) {
    let (fw, fh) = font_size;
    (
        area.width.saturating_mul(fw).max(1),
        area.height.saturating_mul(fh).max(1),
    )
}

fn render_current_page(
    store: &PdfStore,
    app: &App,
    picker: &Picker,
    page_state: &mut PageViewState,
    image_cache: &mut ImageCache,
    term_size: Rect,
    font_size: (u16, u16),
) {
    // In spread mode, render the left page from spread_pages()
    let (left_page, _) = app.spread_pages();
    let page_idx = match left_page {
        Some(idx) => idx,
        None => {
            // No left page (e.g., Book mode page 0) — clear left protocol
            page_state.protocol = None;
            page_state.image_size = None;
            page_state.rendered_page = None;
            return;
        }
    };
    if page_state.rendered_page == Some(page_idx) {
        return;
    }

    let slot = match app.workspace.pages.get(page_idx) {
        Some(s) => s,
        None => return,
    };
    let doc_id = slot.source.doc_id;
    let page_num = slot.source.page_num;

    // Compute target resolution based on current layout
    let base_cols = if app.fullscreen {
        term_size.width
    } else {
        term_size.width.saturating_sub(14) // subtract sidebar
    };
    // In spread view, each page gets half the width
    let view_cols = if app.spread_mode != crate::app::SpreadMode::Off { base_cols / 2 } else { base_cols };
    let view_rows = if app.fullscreen {
        term_size.height
    } else {
        match app.layout_mode {
            LayoutMode::NoThumbnails => term_size.height.saturating_sub(3), // status + hints
            _ => term_size.height.saturating_sub(11), // status + thumbs + hints
        }
    };
    let (max_w, max_h) = area_to_pixels(
        Rect::new(0, 0, view_cols, view_rows),
        font_size,
    );

    // Check image cache (includes resolution in key)
    if let Some(cached) = image_cache.get(doc_id, page_num, max_w, max_h) {
        page_state.image_size = Some((cached.width(), cached.height()));
        let proto = picker.new_resize_protocol(cached.clone());
        page_state.protocol = Some(proto);
        page_state.rendered_page = Some(page_idx);
        return;
    }

    let pdf = match store.get(doc_id) {
        Some(p) => p,
        None => return,
    };

    let cache = RenderCache::new();
    match renderer::render_page_with_cache(pdf, page_num, 2.0, Some(max_w), Some(max_h), &cache) {
        Ok(image) => {
            page_state.image_size = Some((image.width(), image.height()));
            let proto = picker.new_resize_protocol(image.clone());
            image_cache.put(doc_id, page_num, max_w, max_h, image);
            page_state.protocol = Some(proto);
            page_state.rendered_page = Some(page_idx);
        }
        Err(_) => {
            page_state.protocol = None;
            page_state.image_size = None;
            page_state.rendered_page = None;
        }
    }
}

/// Render the right (next) page for two-page spread view.
fn render_spread_page(
    store: &PdfStore,
    app: &App,
    picker: &Picker,
    page_state: &mut PageViewState,
    image_cache: &mut ImageCache,
    term_size: Rect,
    font_size: (u16, u16),
) {
    if app.spread_mode == crate::app::SpreadMode::Off {
        page_state.protocol_right = None;
        page_state.image_size_right = None;
        page_state.rendered_page_right = None;
        return;
    }

    let (_, right_page) = app.spread_pages();
    let right_idx = match right_page {
        Some(idx) => idx,
        None => {
            page_state.protocol_right = None;
            page_state.image_size_right = None;
            page_state.rendered_page_right = None;
            return;
        }
    };

    if page_state.rendered_page_right == Some(right_idx) {
        return;
    }

    let slot = match app.workspace.pages.get(right_idx) {
        Some(s) => s,
        None => return,
    };
    let doc_id = slot.source.doc_id;
    let page_num = slot.source.page_num;

    // Compute target resolution: half the width for spread view
    let (view_cols, view_rows) = match app.layout_mode {
        LayoutMode::NoThumbnails => (
            term_size.width.saturating_sub(14) / 2,
            term_size.height.saturating_sub(3),
        ),
        _ => (
            term_size.width.saturating_sub(14) / 2,
            term_size.height.saturating_sub(11),
        ),
    };
    let (max_w, max_h) = area_to_pixels(
        Rect::new(0, 0, view_cols, view_rows),
        font_size,
    );

    if let Some(cached) = image_cache.get(doc_id, page_num, max_w, max_h) {
        page_state.image_size_right = Some((cached.width(), cached.height()));
        let proto = picker.new_resize_protocol(cached.clone());
        page_state.protocol_right = Some(proto);
        page_state.rendered_page_right = Some(right_idx);
        return;
    }

    let pdf = match store.get(doc_id) {
        Some(p) => p,
        None => return,
    };

    let cache = RenderCache::new();
    match renderer::render_page_with_cache(pdf, page_num, 2.0, Some(max_w), Some(max_h), &cache) {
        Ok(image) => {
            page_state.image_size_right = Some((image.width(), image.height()));
            let proto = picker.new_resize_protocol(image.clone());
            image_cache.put(doc_id, page_num, max_w, max_h, image);
            page_state.protocol_right = Some(proto);
            page_state.rendered_page_right = Some(right_idx);
        }
        Err(_) => {
            page_state.protocol_right = None;
            page_state.image_size_right = None;
            page_state.rendered_page_right = None;
        }
    }
}

/// Render up to `batch_size` missing nearby thumbnails.
/// Returns true if any were rendered (caller should redraw soon).
fn render_nearby_thumbnails(
    store: &PdfStore,
    app: &App,
    thumb_state: &mut ThumbnailBarState,
    font_size: (u16, u16),
    term_size: Rect,
    batch_size: usize,
) -> bool {
    let page_count = app.page_count();
    thumb_state.ensure_capacity(page_count);

    let current = app.current_page();

    // Compute visible range based on layout mode
    let (start, end) = visible_thumb_range(app, current, page_count, term_size);

    // Evict thumbnails far outside the visible window to bound memory use.
    // Keep a small buffer beyond the visible range for smooth scrolling.
    let evict_start = start.saturating_sub(10);
    let evict_end = (end + 10).min(page_count);
    thumb_state.evict_outside(evict_start, evict_end);

    // Thumbnail area is ~8 rows tall
    let (_, max_th) = area_to_pixels(Rect::new(0, 0, 20, 8), font_size);

    let mut rendered = 0;

    // Render thumbnails grouped by doc_id to share hayro cache
    for doc_id in 0..store.pdfs.len() {
        if rendered >= batch_size {
            break;
        }
        let pdf = &store.pdfs[doc_id];
        let cache = RenderCache::new();

        for page_idx in start..end {
            if rendered >= batch_size {
                break;
            }
            if thumb_state.is_rendered(page_idx) {
                continue;
            }

            let slot = match app.workspace.pages.get(page_idx) {
                Some(s) if s.source.doc_id == doc_id => s,
                _ => continue,
            };

            if let Ok(image) = renderer::render_page_with_cache(
                pdf, slot.source.page_num, 0.5, Some(max_th), Some(max_th), &cache,
            ) {
                if page_idx < thumb_state.images.len() {
                    thumb_state.images[page_idx] = Some(image);
                    rendered += 1;
                }
            }
        }
    }

    rendered > 0
}

/// Execute a text search across all pages.
fn execute_search(
    store: &PdfStore,
    page_count: usize,
    workspace: &crate::model::workspace::Workspace,
    search: &mut crate::app::SearchState,
) {
    let query_lower = search.query.to_lowercase();
    search.matches.clear();

    for page_idx in 0..page_count {
        let slot = match workspace.pages.get(page_idx) {
            Some(s) => s,
            None => continue,
        };
        let pdf = match store.get(slot.source.doc_id) {
            Some(p) => p,
            None => continue,
        };

        if let Ok(text) = text_layout::extract_text(pdf, slot.source.page_num) {
            let text_lower = text.to_lowercase();
            let count = text_lower.matches(&query_lower).count();
            if count > 0 {
                search.matches.push((page_idx, count));
            }
        }
    }
}

/// Extract text for the current page and store in PageViewState.
/// Update search match y-positions for the current page.
fn update_match_positions(store: &PdfStore, app: &mut App) {
    let query = match &app.search {
        Some(s) if !s.query.is_empty() => s.query.clone(),
        _ => {
            if let Some(ref mut s) = app.search {
                s.current_page_match_positions.clear();
            }
            return;
        }
    };

    let page_idx = app.current_page();
    let slot = match app.workspace.pages.get(page_idx) {
        Some(s) => s,
        None => return,
    };
    let pdf = match store.get(slot.source.doc_id) {
        Some(p) => p,
        None => return,
    };

    let positions = text_layout::find_match_positions(pdf, slot.source.page_num, &query)
        .unwrap_or_default();

    if let Some(ref mut s) = app.search {
        s.current_page_match_positions = positions;
    }
}

fn extract_current_text(
    store: &PdfStore,
    app: &App,
    page_state: &mut PageViewState,
    term_size: Rect,
) {
    let page_idx = app.current_page();
    // Already cached for this page?
    if let Some((cached, _)) = &page_state.text_lines {
        if *cached == page_idx {
            return;
        }
    }

    let slot = match app.workspace.pages.get(page_idx) {
        Some(s) => s,
        None => return,
    };
    let pdf = match store.get(slot.source.doc_id) {
        Some(p) => p,
        None => return,
    };

    // Available text area: full width minus sidebar, minus status/hints rows
    let cols = term_size.width.saturating_sub(14);
    let rows = match app.layout_mode {
        LayoutMode::NoThumbnails => term_size.height.saturating_sub(3),
        _ => term_size.height.saturating_sub(11),
    };

    match text_layout::extract_text_grid(pdf, slot.source.page_num, cols, rows) {
        Ok(lines) => {
            page_state.text_lines = Some((page_idx, lines));
        }
        Err(_) => {
            page_state.text_lines = Some((page_idx, vec!["  Failed to extract text.".into()]));
        }
    }
}

/// Compute the range of page indices that are visible in the thumbnail area.
fn visible_thumb_range(app: &App, current: usize, page_count: usize, term_size: Rect) -> (usize, usize) {
    if app.layout_mode == LayoutMode::ThumbnailsOnly {
        let cell_h = 8u16;
        let cell_w = ((cell_h as f32) * 1.4).ceil() as u16;
        let grid_w = term_size.width.saturating_sub(13); // minus sidebar
        let cols = (grid_w / (cell_w + 1)).max(1) as usize;
        let rows = (term_size.height.saturating_sub(2) / cell_h).max(1) as usize;
        let per_screen = cols * rows;
        let screen_start = (current / per_screen) * per_screen;
        let screen_end = (screen_start + per_screen).min(page_count);
        (screen_start, screen_end)
    } else {
        // Strip mode: ±4 around current
        let start = current.saturating_sub(4);
        let end = (current + 5).min(page_count);
        (start, end)
    }
}

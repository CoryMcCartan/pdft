use crate::app::{App, LayoutMode, Mode, SearchState, SpreadMode, TextScroll, ViewMode};
use crate::tui::views::dialog::InputDialog;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind, MouseButton};
use ratatui::layout::Rect;

/// Parse a single coordinate value. Supports:
/// - "50%" → percentage of page dimension
/// - "1.5in" → inches (converted to points at 72pt/in)
/// - "1.5" → bare number, interpreted as inches
/// Returns the resolved value in PDF points given the page dimension in points.
fn parse_coord(s: &str, page_dim: f64) -> Option<f64> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let v: f64 = pct.trim().parse().ok()?;
        if !(0.0..=100.0).contains(&v) {
            return None;
        }
        Some(v / 100.0 * page_dim)
    } else if let Some(inches) = s.strip_suffix("in") {
        let v: f64 = inches.trim().parse().ok()?;
        let pts = v * 72.0;
        if pts < 0.0 || pts > page_dim {
            return None;
        }
        Some(pts)
    } else {
        // bare number → inches
        let v: f64 = s.parse().ok()?;
        let pts = v * 72.0;
        if pts < 0.0 || pts > page_dim {
            return None;
        }
        Some(pts)
    }
}

/// Handle a key event and update the app state.
/// Returns true if the event was consumed.
pub fn handle_key(app: &mut App, key: KeyEvent, dialog: &mut InputDialog) -> bool {
    // Ctrl+C always quits
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.should_quit = true;
        return true;
    }

    // Dialog mode input handling
    if dialog.active {
        match key.code {
            KeyCode::Esc => {
                dialog.close();
                app.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                let input = dialog.take_input();
                handle_dialog_submit(app, &input, dialog);
            }
            KeyCode::Backspace => {
                dialog.backspace();
            }
            KeyCode::Char(c) => {
                dialog.insert_char(c);
            }
            _ => {}
        }
        return true;
    }

    if app.show_help {
        app.show_help = false;
        return true;
    }

    // Fullscreen mode: only page navigation and exit
    if app.fullscreen {
        match key.code {
            KeyCode::Char('z') | KeyCode::Esc => {
                app.fullscreen = false;
            }
            KeyCode::Char('q') => {
                app.should_quit = true;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if app.spread_mode != SpreadMode::Off {
                    app.next_page();
                    app.next_page();
                } else {
                    app.next_page();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if app.spread_mode != SpreadMode::Off {
                    app.prev_page();
                    app.prev_page();
                } else {
                    app.prev_page();
                }
            }
            KeyCode::Char('g') if app.pending_g => {
                app.workspace.selected_page = 0;
                app.pending_g = false;
            }
            KeyCode::Char('g') => {
                app.pending_g = true;
            }
            KeyCode::Char('G') => {
                app.workspace.selected_page = app.page_count().saturating_sub(1);
                app.pending_g = false;
            }
            KeyCode::Char(':') => {
                app.mode = Mode::GotoPage;
                dialog.title = "Go to".into();
                dialog.prompt = "Page:".into();
                dialog.open();
            }
            KeyCode::Char('2') => {
                app.spread_mode = match app.spread_mode {
                    SpreadMode::Off => SpreadMode::Book,
                    SpreadMode::Book => SpreadMode::Paired,
                    SpreadMode::Paired => SpreadMode::Off,
                };
            }
            _ => {
                app.pending_g = false;
            }
        }
        return true;
    }

    // Form filling mode: Tab/Shift-Tab/Enter/Esc
    if app.mode == Mode::FormFilling {
        match key.code {
            KeyCode::Char('q') => {
                app.mode = Mode::Normal;
                app.form_fields.clear();
                app.should_quit = true;
            }
            KeyCode::Esc => {
                app.mode = Mode::Normal;
                app.form_fields.clear();
                app.status_message = Some("Exited form mode".into());
            }
            KeyCode::Tab => {
                if !app.form_fields.is_empty() {
                    app.form_field_index = (app.form_field_index + 1) % app.form_fields.len();
                    let field = &app.form_fields[app.form_field_index];
                    app.workspace.selected_page = field.page_num;
                    app.status_message = Some(format!("Field: {}", field.name));
                }
            }
            KeyCode::BackTab => {
                if !app.form_fields.is_empty() {
                    if app.form_field_index == 0 {
                        app.form_field_index = app.form_fields.len() - 1;
                    } else {
                        app.form_field_index -= 1;
                    }
                    let field = &app.form_fields[app.form_field_index];
                    app.workspace.selected_page = field.page_num;
                    app.status_message = Some(format!("Field: {}", field.name));
                }
            }
            KeyCode::Enter => {
                if !app.form_fields.is_empty() {
                    open_form_field_dialog(app, dialog);
                }
            }
            _ => {}
        }
        return true;
    }

    // Text placing mode: Esc cancels, : opens coordinate dialog
    if app.mode == Mode::TextPlacing {
        match key.code {
            KeyCode::Esc => {
                app.mode = Mode::Normal;
                app.status_message = Some("Text placement cancelled".into());
            }
            KeyCode::Char(':') => {
                app.mode = Mode::TextPositionInput;
                dialog.title = "Add text".into();
                dialog.prompt = "Position x,y (e.g. 50%,80% or 1in,7in or 1,7):".into();
                dialog.open();
            }
            _ => {}
        }
        return true;
    }

    // Signature placing mode: Esc cancels, : opens coordinate dialog
    if app.mode == Mode::SignaturePlacing {
        match key.code {
            KeyCode::Esc => {
                app.mode = Mode::Normal;
                app.status_message = Some("Signature placement cancelled".into());
            }
            KeyCode::Char(':') => {
                app.mode = Mode::SignaturePositionInput;
                dialog.title = "Signature".into();
                dialog.prompt = "Position x,y (e.g. 50%,80% or 1in,7in or 1,7):".into();
                dialog.open();
            }
            _ => {}
        }
        return true;
    }

    // Pending g: next key determines action (g = first page)
    if app.pending_g {
        app.pending_g = false;
        if let KeyCode::Char('g') = key.code {
            app.workspace.selected_page = 0;
        }
        // Any other key after g is ignored (consumed)
        return true;
    }

    // Pending assign: next key must be a-z group letter
    if app.pending_assign {
        app.pending_assign = false;
        if let KeyCode::Char(c) = key.code {
            if c.is_ascii_lowercase() {
                let (start, end) = app.selected_range();
                let target_id = app.get_or_create_group(c);
                let indices: Vec<usize> = (start..=end).collect();

                // Toggle: if ALL selected pages already have this group, clear it
                let all_have_group = indices
                    .iter()
                    .all(|&i| app.workspace.pages[i].output_target == Some(target_id));

                if all_have_group {
                    app.workspace.assign_output(&indices, None);
                    app.status_message = Some(format!(
                        "Cleared group '{c}' from {} page(s)",
                        indices.len()
                    ));
                } else {
                    app.workspace.assign_output(&indices, Some(target_id));
                    // Clear deletion marks — assign and delete are mutually exclusive
                    for &i in &indices {
                        if app.workspace.pages[i].marked_for_delete {
                            app.workspace.toggle_delete(i);
                        }
                    }
                    app.status_message = Some(format!(
                        "Assigned {} page(s) to group '{c}'",
                        indices.len()
                    ));
                }
                app.visual_anchor = None;
                return true;
            }
        }
        app.status_message = Some("Assign cancelled".into());
        return true;
    }

    match key.code {
        KeyCode::Char('q') => {
            app.should_quit = true;
        }
        KeyCode::Esc => {
            // Esc exits visual mode, or quits if not in visual
            if app.visual_anchor.is_some() {
                app.visual_anchor = None;
                app.pending_assign = false;
            } else {
                app.should_quit = true;
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if app.spread_mode != SpreadMode::Off {
                app.next_page();
                app.next_page();
            } else {
                app.next_page();
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.spread_mode != SpreadMode::Off {
                app.prev_page();
                app.prev_page();
            } else {
                app.prev_page();
            }
        }
        KeyCode::Char('h') | KeyCode::Left => {
            if app.view_mode == ViewMode::Text {
                app.text_scroll = TextScroll::Lines(-3);
            }
        }
        KeyCode::Char('l') | KeyCode::Right => {
            if app.view_mode == ViewMode::Text {
                app.text_scroll = TextScroll::Lines(3);
            }
        }
        KeyCode::Char('d') | KeyCode::Char('x') => {
            // Toggle delete on selected range; clear any group assignments
            let (start, end) = app.selected_range();
            let indices: Vec<usize> = (start..=end).collect();
            // Clear assignments — delete and assign are mutually exclusive
            app.workspace.assign_output(&indices, None);
            app.workspace.toggle_delete_batch(&indices);
            app.visual_anchor = None;
        }
        KeyCode::Char('a') => {
            // Start assign: next key is the group letter
            app.pending_assign = true;
            app.status_message = Some("Assign to group: a-z".into());
        }
        KeyCode::Char('v') => {
            // Toggle visual selection mode
            if app.visual_anchor.is_some() {
                app.visual_anchor = None;
            } else {
                app.visual_anchor = Some(app.current_page());
            }
        }
        KeyCode::Char('s') => {
            if app.workspace.documents.is_empty() {
                app.status_message = Some("No documents loaded".into());
            } else {
                let has_groups = app.workspace.pages.iter().any(|p| p.output_target.is_some());
                if has_groups {
                    // Go straight to per-group prompts using original path as base
                    let base = app.original_path().unwrap_or_default();
                    app.prepare_group_saves(&base);
                    prompt_next_group_save(app, dialog);
                } else {
                    app.mode = Mode::SaveInput;
                    dialog.title = "Save".into();
                    dialog.prompt = "Save to:".into();
                    let path = app
                        .original_path()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    dialog.open_with(&path);
                }
            }
        }
        KeyCode::Char('m') => {
            app.mode = Mode::MergeInput;
            dialog.title = "Merge PDF".into();
            dialog.prompt = "Enter file path:".into();
            dialog.open();
        }
        KeyCode::Char(':') => {
            app.mode = Mode::GotoPage;
            dialog.title = "Go to".into();
            dialog.prompt = "Page:".into();
            dialog.open();
        }
        KeyCode::Char('/') => {
            app.mode = Mode::SearchInput;
            dialog.title = "Search".into();
            dialog.prompt = "Search text:".into();
            dialog.open();
        }
        KeyCode::Char('n') => {
            search_next(app, false);
        }
        KeyCode::Char('N') => {
            search_next(app, true);
        }
        KeyCode::Char('u') => {
            // Try form field undos first (if in form mode), then doc-level, then workspace
            if app.mode == Mode::FormFilling {
                if let Some(undo) = app.form_field_undos.pop() {
                    match app.workspace.documents[0].set_form_field_value(undo.obj_id, &undo.old_value) {
                        Ok(_) => {
                            if undo.field_index < app.form_fields.len() {
                                app.form_fields[undo.field_index].value = undo.old_value;
                            }
                            app.form_dirty = true;
                            app.status_message = Some("Form field change undone".into());
                        }
                        Err(e) => {
                            app.status_message = Some(format!("Undo error: {e}"));
                        }
                    }
                } else {
                    app.status_message = Some("Nothing to undo".into());
                }
            } else {
                // Try document-level undos first (text stamps, signatures), then workspace undo
                let has_doc_undo = app.workspace.documents.iter()
                    .any(|d| !d.signature_undos.is_empty() || !d.text_stamp_undos.is_empty());
                if has_doc_undo {
                    app.pending_signature_undo = true; // reuse flag for both types
                } else if !app.undo() {
                    app.status_message = Some("Nothing to undo".into());
                }
            }
        }
        KeyCode::Char('t') => {
            app.toggle_view_mode();
        }
        KeyCode::Char('w') => {
            app.cycle_layout();
        }
        KeyCode::Char('z') => {
            app.toggle_fullscreen();
        }
        KeyCode::Char('2') => {
            app.spread_mode = match app.spread_mode {
                SpreadMode::Off => SpreadMode::Book,
                SpreadMode::Book => SpreadMode::Paired,
                SpreadMode::Paired => SpreadMode::Off,
            };
            app.status_message = Some(match app.spread_mode {
                SpreadMode::Off => "Single page view".into(),
                SpreadMode::Book => "Book spread (cover + pairs)".into(),
                SpreadMode::Paired => "Paired spread (1-2, 3-4, ...)".into(),
            });
        }
        KeyCode::Char(']') => {
            app.next_comment_page();
        }
        KeyCode::Char('[') => {
            app.prev_comment_page();
        }
        KeyCode::Char('F') => {
            if app.workspace.documents.is_empty() {
                app.status_message = Some("No documents loaded".into());
            } else {
                match app.workspace.documents[0].extract_form_fields() {
                    Ok(fields) => {
                        if fields.is_empty() {
                            app.status_message = Some("No form fields found".into());
                        } else {
                            let page = fields[0].page_num;
                            app.form_fields = fields;
                            app.form_field_index = 0;
                            app.form_field_undos.clear();
                            app.mode = Mode::FormFilling;
                            app.workspace.selected_page = page;
                            app.status_message = Some(format!(
                                "Form mode: {} field(s)",
                                app.form_fields.len()
                            ));
                        }
                    }
                    Err(e) => {
                        app.status_message = Some(format!("{e}"));
                    }
                }
            }
        }
        KeyCode::Char('A') => {
            if app.workspace.documents.is_empty() {
                app.status_message = Some("No documents loaded".into());
            } else {
                app.mode = Mode::TextPlacing;
                app.status_message = Some("Click on page to place text, or : for coordinates".into());
            }
        }
        KeyCode::Char('S') => {
            if app.workspace.documents.is_empty() {
                app.status_message = Some("No documents loaded".into());
            } else {
                // Try env var if no path cached
                if app.signature_path.is_none() {
                    if let Ok(path) = std::env::var("PDFT_SIGNATURE") {
                        let p = std::path::PathBuf::from(&path);
                        if p.exists() {
                            app.signature_path = Some(p);
                        } else {
                            app.status_message = Some(format!("PDFT_SIGNATURE file not found: {path}"));
                            return true;
                        }
                    }
                }

                if app.signature_path.is_some() {
                    app.mode = Mode::SignaturePlacing;
                    app.status_message = Some("Click on page to place signature, or : for coordinates".into());
                } else {
                    app.mode = Mode::SignaturePathInput;
                    dialog.title = "Signature".into();
                    dialog.prompt = "Path to PNG:".into();
                    dialog.open();
                }
            }
        }
        KeyCode::Char('?') => {
            app.show_help = true;
        }
        KeyCode::Char('G') => {
            let last = app.page_count().saturating_sub(1);
            app.workspace.selected_page = last;
        }
        KeyCode::Char('g') => {
            app.pending_g = true;
        }
        KeyCode::Home => {
            app.workspace.selected_page = 0;
        }
        KeyCode::End => {
            let last = app.page_count().saturating_sub(1);
            app.workspace.selected_page = last;
        }
        KeyCode::PageDown => {
            if app.view_mode == ViewMode::Text {
                app.text_scroll = TextScroll::Lines(20);
            }
        }
        KeyCode::PageUp => {
            if app.view_mode == ViewMode::Text {
                app.text_scroll = TextScroll::Lines(-20);
            }
        }
        _ => return false,
    }

    true
}

fn handle_dialog_submit(
    app: &mut App,
    input: &str,
    dialog: &mut crate::tui::views::dialog::InputDialog,
) {
    match app.mode {
        Mode::MergeInput => {
            let path = std::path::Path::new(input.trim());
            match app.workspace.open(path) {
                Ok(_) => {
                    app.status_message = Some(format!(
                        "Merged {} ({} total pages)",
                        path.display(),
                        app.page_count()
                    ));
                }
                Err(e) => {
                    app.status_message = Some(format!("Merge error: {e}"));
                }
            }
        }
        Mode::SaveInput => {
            let path = std::path::PathBuf::from(input.trim());
            if path.exists() {
                app.pending_save_path = Some(path);
                app.mode = Mode::SaveConfirm;
                dialog.prompt = "File exists. Overwrite? (y/n)".into();
                dialog.open_with("");
                return;
            } else {
                match app.save_to(&path) {
                    Ok(()) => {}
                    Err(e) => {
                        app.status_message = Some(format!("Save error: {e}"));
                    }
                }
            }
        }
        Mode::SaveConfirm => {
            let answer = input.trim().to_lowercase();
            if answer == "y" || answer == "yes" {
                if let Some(path) = app.pending_save_path.take() {
                    match app.save_to(&path) {
                        Ok(()) => {}
                        Err(e) => {
                            app.status_message = Some(format!("Save error: {e}"));
                        }
                    }
                }
            } else {
                app.pending_save_path = None;
                app.pending_group_saves.clear();
                app.status_message = Some("Save cancelled".into());
            }
        }
        Mode::SaveGroupInput => {
            let path = std::path::PathBuf::from(input.trim());
            // Save this group
            if let Some((label, pages, _)) = app.pending_group_saves.first() {
                let label = label.clone();
                let pages = pages.clone();
                match app.save_group(&pages, &path) {
                    Ok(()) => {
                        app.status_message = Some(format!(
                            "Saved {label} ({} pages) → {}",
                            pages.len(),
                            path.display()
                        ));
                    }
                    Err(e) => {
                        app.status_message = Some(format!("Save error: {e}"));
                        app.pending_group_saves.clear();
                        app.mode = Mode::Normal;
                        return;
                    }
                }
            }
            app.pending_group_saves.remove(0);
            if !app.pending_group_saves.is_empty() {
                prompt_next_group_save(app, dialog);
                return;
            } else {
                // All groups saved, clear marks
                for page in &mut app.workspace.pages {
                    page.marked_for_delete = false;
                }
            }
        }
        Mode::GotoPage => {
            let input = input.trim();
            if let Ok(n) = input.parse::<usize>() {
                if n >= 1 && n <= app.page_count() {
                    app.workspace.selected_page = n - 1;
                } else {
                    app.status_message =
                        Some(format!("Page {n} out of range (1-{})", app.page_count()));
                }
            } else if !input.is_empty() {
                app.status_message = Some("Invalid page number".into());
            }
        }
        Mode::TextPositionInput => {
            let parts: Vec<&str> = input.trim().split(',').collect();
            if parts.len() == 2 {
                let page_idx = app.current_page();
                let dims = app.workspace.documents.first()
                    .map(|d| d.page_dimensions())
                    .unwrap_or_default();
                if let Some(&(pw, ph)) = dims.get(page_idx) {
                    if let (Some(pdf_x), Some(raw_y)) = (parse_coord(parts[0], pw), parse_coord(parts[1], ph)) {
                        // y is specified from top, convert to PDF coords (from bottom)
                        let pdf_y = ph - raw_y;
                        app.pending_text_stamp = Some((pdf_x, pdf_y));
                        app.mode = Mode::TextContentInput;
                        dialog.title = "Add text".into();
                        dialog.prompt = "Text:".into();
                        dialog.open();
                        return;
                    }
                }
            }
            app.status_message = Some("Invalid position. Use: x,y (e.g. 50%,80% or 1in,7in or 1,7)".into());
        }
        Mode::TextContentInput => {
            let text = input.trim().to_string();
            if text.is_empty() {
                app.pending_text_stamp = None;
                app.status_message = Some("Text placement cancelled".into());
            } else if let Some((pdf_x, pdf_y)) = app.pending_text_stamp.take() {
                let page_idx = app.current_page();
                let slot = &app.workspace.pages[page_idx];
                let doc_id = slot.source.doc_id;
                let page_num = slot.source.page_num;
                let font_size = app.text_stamp_size_pt;

                match app.workspace.documents[doc_id].embed_text(page_num, &text, pdf_x, pdf_y, font_size) {
                    Ok(()) => {
                        app.needs_pdf_refresh = true;
                        app.status_message = Some("Text placed. Save to keep changes.".into());
                    }
                    Err(e) => {
                        app.status_message = Some(format!("Text error: {e}"));
                    }
                }
            }
        }
        Mode::SignaturePathInput => {
            let path = std::path::PathBuf::from(input.trim());
            if path.exists() {
                app.signature_path = Some(path);
                app.mode = Mode::SignaturePlacing;
                app.status_message = Some("Click on page to place signature, or : for coordinates".into());
                return; // don't reset to Normal
            } else {
                app.status_message = Some(format!("File not found: {}", path.display()));
            }
        }
        Mode::SignaturePositionInput => {
            let parts: Vec<&str> = input.trim().split(',').collect();
            if parts.len() == 2 {
                let page_idx = app.current_page();
                let dims = app.workspace.documents.first()
                    .map(|d| d.page_dimensions())
                    .unwrap_or_default();
                if let Some(&(pw, ph)) = dims.get(page_idx) {
                    if let (Some(pdf_x), Some(raw_y)) = (parse_coord(parts[0], pw), parse_coord(parts[1], ph)) {
                        // y is specified from top, convert to PDF coords (from bottom)
                        let pdf_y = ph - raw_y;
                        app.pending_signature = Some((pdf_x, pdf_y));
                    } else {
                        app.status_message = Some("Invalid position. Use: x,y (e.g. 50%,80% or 1in,7in or 1,7)".into());
                    }
                } else {
                    app.status_message = Some("Could not determine page dimensions".into());
                }
            } else {
                app.status_message = Some("Invalid position. Use: x,y (e.g. 50%,80% or 1in,7in or 1,7)".into());
            }
        }
        Mode::FormFieldInput => {
            let new_value = input.trim().to_string();
            let idx = app.form_field_index;
            if idx < app.form_fields.len() {
                let field = &app.form_fields[idx];
                let obj_id = field.obj_id;
                let doc_id = 0; // form fields come from first document
                match app.workspace.documents[doc_id].set_form_field_value(obj_id, &new_value) {
                    Ok(old_value) => {
                        app.form_field_undos.push(crate::model::document::FormFieldUndo {
                            obj_id,
                            old_value,
                            field_index: idx,
                        });
                        app.form_fields[idx].value = new_value;
                        app.form_dirty = true;
                        app.needs_pdf_refresh = true;
                        app.status_message = Some(format!("Set {}", app.form_fields[idx].name));
                    }
                    Err(e) => {
                        app.status_message = Some(format!("Form error: {e}"));
                    }
                }
            }
            app.mode = Mode::FormFilling;
            return;
        }
        Mode::SearchInput => {
            let query = input.trim().to_string();
            if query.is_empty() {
                app.search = None;
                app.status_message = Some("Search cleared".into());
            } else {
                app.search = Some(SearchState {
                    query,
                    matches: Vec::new(),
                    current_match: 0,
                    current_page_match_positions: Vec::new(),
                });
            }
        }
        _ => {}
    }
    app.mode = Mode::Normal;
}

/// Open the form field edit dialog with appropriate prompt for the field type.
pub fn open_form_field_dialog(app: &mut App, dialog: &mut crate::tui::views::dialog::InputDialog) {
    let field = &app.form_fields[app.form_field_index];
    app.mode = Mode::FormFieldInput;
    dialog.title = "Form field".into();

    use crate::model::document::FormFieldType;
    match field.field_type {
        FormFieldType::Checkbox => {
            if field.options.is_empty() {
                dialog.prompt = format!("{} (Yes/Off):", field.name);
            } else {
                let opts = field.options.join("/");
                dialog.prompt = format!("{} ({}/Off):", field.name, opts);
            }
            dialog.open_with(&field.value);
        }
        FormFieldType::Choice => {
            if field.options.is_empty() {
                dialog.prompt = format!("{}:", field.name);
            } else {
                let opts = field.options.join(", ");
                dialog.prompt = format!("{} [{}]:", field.name, opts);
            }
            dialog.open_with(&field.value);
        }
        FormFieldType::Text => {
            dialog.prompt = format!("{}:", field.name);
            dialog.open_with(&field.value);
        }
    }
}

fn prompt_next_group_save(app: &mut App, dialog: &mut crate::tui::views::dialog::InputDialog) {
    if let Some((label, pages, default_path)) = app.pending_group_saves.first() {
        app.mode = Mode::SaveGroupInput;
        dialog.title = "Save".into();
        dialog.prompt = format!("Save {} ({} pages):", label, pages.len());
        dialog.open_with(&default_path.to_string_lossy());
    }
}

fn search_next(app: &mut App, reverse: bool) {
    let cur_page = app.workspace.selected_page;

    let search = match &mut app.search {
        Some(s) if !s.matches.is_empty() => s,
        _ => {
            app.status_message = Some("No search results".into());
            return;
        }
    };

    let len = search.matches.len();

    if reverse {
        // Find the last match on a page before (or equal to) current page,
        // or wrap to the last match in the document.
        search.current_match = search
            .matches
            .iter()
            .rposition(|&(p, _)| p < cur_page)
            .unwrap_or(len - 1);
    } else {
        // Find the first match on a page after current page,
        // or wrap to the first match in the document.
        search.current_match = search
            .matches
            .iter()
            .position(|&(p, _)| p > cur_page)
            .unwrap_or(0);
    }

    let (page_idx, _count) = search.matches[search.current_match];
    app.workspace.selected_page = page_idx;
    app.scroll_to_match = true;
    let pos = search.current_match + 1;
    let total = search.matches.len();
    app.status_message = Some(format!("Match {pos}/{total}"));
}

/// Handle a mouse event. `term_size` is needed to compute layout regions.
/// `image_area` and `page_dims` are used for signature click-to-place.
/// Returns true if the event was consumed and needs a rerender.
pub fn handle_mouse(
    app: &mut App,
    mouse: MouseEvent,
    term_size: Rect,
    image_area: Option<Rect>,
    page_dims: Option<(f64, f64)>,
) -> bool {
    // In form filling mode, handle clicks on fields
    if app.mode == Mode::FormFilling {
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            if let (Some(img_area), Some((page_w, page_h))) = (image_area, page_dims) {
                let col = mouse.column;
                let row = mouse.row;
                if col >= img_area.x && col < img_area.x + img_area.width
                    && row >= img_area.y && row < img_area.y + img_area.height
                {
                    let rel_x = (col as f64 - img_area.x as f64 + 0.5) / img_area.width as f64;
                    let rel_y = (row as f64 - img_area.y as f64 + 0.5) / img_area.height as f64;
                    let pdf_x = rel_x * page_w;
                    let pdf_y = (1.0 - rel_y) * page_h;
                    let current_page = app.current_page();

                    // Find which field was clicked
                    for (i, field) in app.form_fields.iter().enumerate() {
                        if field.page_num != current_page {
                            continue;
                        }
                        let (x1, y1, x2, y2) = (
                            field.rect[0].min(field.rect[2]),
                            field.rect[1].min(field.rect[3]),
                            field.rect[0].max(field.rect[2]),
                            field.rect[1].max(field.rect[3]),
                        );
                        if pdf_x >= x1 && pdf_x <= x2 && pdf_y >= y1 && pdf_y <= y2 {
                            app.form_field_index = i;
                            // Signal to open dialog (event loop will handle)
                            app.mode = Mode::FormFieldInput;
                            return true;
                        }
                    }
                }
            }
        }
        return false;
    }

    // In text placing mode, handle clicks for placement
    if app.mode == Mode::TextPlacing {
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            if let (Some(img_area), Some((page_w, page_h))) = (image_area, page_dims) {
                let col = mouse.column;
                let row = mouse.row;
                if col >= img_area.x && col < img_area.x + img_area.width
                    && row >= img_area.y && row < img_area.y + img_area.height
                {
                    let rel_x = (col as f64 - img_area.x as f64 + 0.5) / img_area.width as f64;
                    let rel_y = (row as f64 - img_area.y as f64 + 0.5) / img_area.height as f64;
                    let pdf_x = rel_x * page_w;
                    // Click = left-middle of text, offset by half the font size
                    let pdf_y = (1.0 - rel_y) * page_h - app.text_stamp_size_pt / 2.0;
                    app.pending_text_stamp = Some((pdf_x, pdf_y));
                    app.mode = Mode::TextContentInput;
                    // Can't open dialog here (no access), set flag for event loop
                    return true;
                }
            }
        }
        return false;
    }

    // In signature placing mode, handle clicks for placement
    if app.mode == Mode::SignaturePlacing {
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            if let (Some(img_area), Some((page_w, page_h))) = (image_area, page_dims) {
                let col = mouse.column;
                let row = mouse.row;
                if col >= img_area.x && col < img_area.x + img_area.width
                    && row >= img_area.y && row < img_area.y + img_area.height
                {
                    let rel_x = (col as f64 - img_area.x as f64 + 0.5) / img_area.width as f64;
                    let rel_y = (row as f64 - img_area.y as f64 + 0.5) / img_area.height as f64;
                    let pdf_x = rel_x * page_w;
                    // rel_y maps to PDF top → convert to bottom-left origin
                    let click_pdf_y = (1.0 - rel_y) * page_h;
                    // Compute signature height from PNG aspect ratio
                    let sig_h = app.signature_path.as_ref()
                        .and_then(|p| image::image_dimensions(p).ok())
                        .map(|(w, h)| app.signature_width_pt * h as f64 / w as f64)
                        .unwrap_or(app.signature_width_pt * 0.5);
                    // Click = left-middle of signature: offset y down by half height
                    let pdf_y = click_pdf_y - sig_h / 2.0;
                    app.pending_signature = Some((pdf_x, pdf_y));
                    app.mode = Mode::Normal;
                    return true;
                }
            }
        }
        return false;
    }

    // Ignore mouse in dialog/other modes
    if app.mode != Mode::Normal && app.mode != Mode::FormFilling {
        return false;
    }

    let sidebar_width: u16 = 12;
    let status_bar_height: u16 = 1;
    let hints_height: u16 = 1;
    let thumb_height: u16 = if app.layout_mode != LayoutMode::NoThumbnails
        && app.layout_mode != LayoutMode::ThumbnailsOnly
    {
        8
    } else {
        0
    };

    match mouse.kind {
        MouseEventKind::ScrollUp => {
            if app.view_mode == ViewMode::Text && mouse.column >= sidebar_width {
                app.text_scroll = TextScroll::Lines(-3);
                true
            } else {
                false
            }
        }
        MouseEventKind::ScrollDown => {
            if app.view_mode == ViewMode::Text && mouse.column >= sidebar_width {
                app.text_scroll = TextScroll::Lines(3);
                true
            } else {
                false
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let col = mouse.column;
            let row = mouse.row;

            // Click in sidebar region
            if col < sidebar_width && row >= status_bar_height {
                let sidebar_top = status_bar_height;
                let sidebar_bottom = term_size.height.saturating_sub(hints_height + thumb_height);
                // +1 for the block title row (" Pages ")
                let items_top = sidebar_top + 1;
                if row >= items_top && row < sidebar_bottom {
                    let clicked_idx = app.sidebar_offset + (row - items_top) as usize;
                    if clicked_idx < app.page_count() {
                        app.workspace.selected_page = clicked_idx;
                        return true;
                    }
                }
            }

            // Click in thumbnail region (strip mode)
            if app.layout_mode == LayoutMode::Normal
                && row >= term_size.height.saturating_sub(hints_height + thumb_height)
                && row < term_size.height.saturating_sub(hints_height)
            {
                let thumb_cell_w = ((thumb_height as f32) * 1.4).ceil() as u16;
                let area_width = term_size.width;
                let cols = (area_width / (thumb_cell_w + 1)).max(1) as usize;
                let half = cols / 2;
                let current = app.current_page();
                let page_count = app.page_count();
                let start = if current >= half {
                    (current - half).min(page_count.saturating_sub(cols))
                } else {
                    0
                };

                let thumb_idx = col / (thumb_cell_w + 1);
                let clicked_page = start + thumb_idx as usize;
                if clicked_page < page_count {
                    app.workspace.selected_page = clicked_page;
                    return true;
                }
            }

            // Click in thumbnail grid (thumbnails-only mode)
            if app.layout_mode == LayoutMode::ThumbnailsOnly && col >= sidebar_width {
                let grid_x = col - sidebar_width;
                let grid_y = row.saturating_sub(status_bar_height);
                let cell_h: u16 = 8;
                let cell_w = ((cell_h as f32) * 1.4).ceil() as u16;
                let grid_width = term_size.width.saturating_sub(sidebar_width);
                let grid_cols = (grid_width / (cell_w + 1)).max(1) as usize;
                let grid_rows = (term_size.height.saturating_sub(status_bar_height + hints_height) / cell_h).max(1) as usize;
                let per_page = grid_cols * grid_rows;

                let click_col = grid_x / (cell_w + 1);
                let click_row = grid_y / cell_h;
                let current = app.current_page();
                let screen_start = (current / per_page) * per_page;
                let clicked_page = screen_start + (click_row as usize) * grid_cols + click_col as usize;

                if clicked_page < app.page_count() {
                    app.workspace.selected_page = clicked_page;
                    return true;
                }
            }

            false
        }
        _ => false,
    }
}

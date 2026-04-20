use crate::app::{App, Mode, SearchState, TextScroll, ViewMode};
use crate::tui::views::dialog::InputDialog;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
            app.next_page();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.prev_page();
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
            if !app.undo() {
                app.status_message = Some("Nothing to undo".into());
            }
        }
        KeyCode::Char('t') => {
            app.toggle_view_mode();
        }
        KeyCode::Char('w') => {
            app.cycle_layout();
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

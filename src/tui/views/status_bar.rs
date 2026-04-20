use crate::app::{App, ViewMode};
use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let page_info = if app.page_count() > 0 {
        format!(
            " page {}/{} ",
            app.current_page() + 1,
            app.page_count()
        )
    } else {
        " no pages ".to_string()
    };

    let mode = match app.view_mode {
        ViewMode::Image => "[image]",
        ViewMode::Text => "[text]",
    };

    let filename = app
        .workspace
        .documents
        .first()
        .map(|d| d.label.as_str())
        .unwrap_or("pdft");

    let deleted_count = app
        .workspace
        .pages
        .iter()
        .filter(|p| p.marked_for_delete)
        .count();
    let del_info = if deleted_count > 0 {
        format!(" {deleted_count} marked for delete ")
    } else {
        String::new()
    };

    let search_info = if let Some(ref search) = app.search {
        if !search.matches.is_empty() {
            let current_page = app.current_page();
            let page_matches = search
                .matches
                .iter()
                .find(|&&(idx, _)| idx == current_page)
                .map(|&(_, count)| count)
                .unwrap_or(0);
            let pos = search.current_match + 1;
            let total = search.matches.len();
            if page_matches > 0 {
                format!(" /{} [{pos}/{total}, {page_matches} on page]", search.query)
            } else {
                format!(" /{} [{pos}/{total}]", search.query)
            }
        } else if !search.query.is_empty() {
            format!(" /{} [no matches]", search.query)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let status = if let Some(msg) = &app.status_message {
        format!(" {msg}")
    } else {
        String::new()
    };

    let line = Line::from(vec![
        Span::styled(format!(" {filename} "), theme::STATUS),
        Span::styled("│", theme::STATUS),
        Span::styled(page_info, theme::STATUS),
        Span::styled("│", theme::STATUS),
        Span::styled(format!(" {mode} "), theme::STATUS),
        Span::styled(del_info, theme::STATUS),
        Span::styled(search_info, theme::SEARCH_MATCH_PAGE),
        Span::styled(status, theme::STATUS),
        // Pad rest
        Span::styled(
            " ".repeat(area.width.saturating_sub(60) as usize),
            theme::STATUS,
        ),
    ]);

    f.render_widget(Paragraph::new(line), area);
}

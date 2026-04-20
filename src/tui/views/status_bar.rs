use crate::app::{App, Mode, ViewMode};
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

    let mode = match (app.view_mode, app.spread_mode) {
        (ViewMode::Text, _) => "[text]",
        (ViewMode::Image, crate::app::SpreadMode::Off) => "[image]",
        (ViewMode::Image, crate::app::SpreadMode::Book) => "[book]",
        (ViewMode::Image, crate::app::SpreadMode::Paired) => "[spread]",
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

    let form_info = if app.mode == Mode::FormFilling && !app.form_fields.is_empty() {
        let idx = app.form_field_index;
        let field = &app.form_fields[idx];
        let val_preview = if field.value.len() > 30 {
            format!("{}...", &field.value[..field.value.floor_char_boundary(30)])
        } else {
            field.value.clone()
        };
        format!(" [{}:\"{}\" ({}/{})]", field.name, val_preview, idx + 1, app.form_fields.len())
    } else {
        String::new()
    };

    let comment_info = app.comments.get(app.current_page())
        .filter(|c| !c.is_empty())
        .map(|comments| {
            let count = comments.len();
            let preview = &comments[0].text;
            let max_len = 50;
            let truncated = if preview.len() > max_len {
                format!("{}…", &preview[..preview.floor_char_boundary(max_len)])
            } else {
                preview.clone()
            };
            if count == 1 {
                format!(" \"{truncated}\"")
            } else {
                format!(" \"{truncated}\" +{}", count - 1)
            }
        })
        .unwrap_or_default();

    let watch_info = if app.watching { " [watching]" } else { "" };

    let status = if let Some(msg) = &app.status_message {
        format!(" {msg}")
    } else {
        String::new()
    };

    let filename_span = format!(" {filename} ");
    let mode_span = format!(" {mode} ");
    let content_len = filename_span.len() + 1 + page_info.len() + 1
        + mode_span.len() + del_info.len() + search_info.len()
        + form_info.len() + comment_info.len() + watch_info.len() + status.len();
    let pad_len = (area.width as usize).saturating_sub(content_len);

    let line = Line::from(vec![
        Span::styled(filename_span, theme::STATUS),
        Span::styled("│", theme::STATUS),
        Span::styled(page_info, theme::STATUS),
        Span::styled("│", theme::STATUS),
        Span::styled(mode_span, theme::STATUS),
        Span::styled(del_info, theme::STATUS),
        Span::styled(search_info, theme::SEARCH_MATCH_PAGE),
        Span::styled(form_info, theme::FORM_FIELD_ACTIVE),
        Span::styled(comment_info, theme::COMMENT_INFO),
        Span::styled(watch_info.to_string(), theme::STATUS),
        Span::styled(status, theme::STATUS),
        Span::styled(" ".repeat(pad_len), theme::STATUS),
    ]);

    f.render_widget(Paragraph::new(line), area);
}

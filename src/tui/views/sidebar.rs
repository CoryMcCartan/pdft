use crate::app::App;
use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

const VISUAL_STYLE: Style = Style::new().fg(Color::White).bg(Color::DarkGray);

pub fn render(f: &mut Frame, area: Rect, app: &mut App) {
    // Build a set of page indices that have search matches
    let match_pages: std::collections::HashSet<usize> = app
        .search
        .as_ref()
        .map(|s| s.matches.iter().map(|&(idx, _)| idx).collect())
        .unwrap_or_default();

    let items: Vec<ListItem> = app
        .workspace
        .pages
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let selected = {
                let (left, right) = app.spread_pages();
                left == Some(i) || right == Some(i)
            };
            let in_visual = app.visual_anchor.is_some() && app.is_selected(i);
            let deleted = slot.marked_for_delete;
            let has_match = match_pages.contains(&i);
            let has_comment = app.comments.get(i).is_some_and(|c| !c.is_empty());

            let marker = if selected { ">" } else { " " };
            let del_marker = if deleted { "×" } else { " " };

            // Show group label if assigned
            let group_label = match slot.output_target {
                Some(id) => app
                    .workspace
                    .output_targets
                    .get(id)
                    .map(|t| format!(" {}", t.label))
                    .unwrap_or_default(),
                None => String::new(),
            };

            let comment_marker = if has_comment { "●" } else { "" };
            let text = format!("{marker}{del_marker}{:>3}{group_label}{comment_marker}", i + 1);

            let style = if selected {
                theme::SELECTED
            } else if in_visual {
                VISUAL_STYLE
            } else if deleted {
                theme::DELETED
            } else if has_match {
                theme::SEARCH_MATCH_PAGE
            } else if has_comment {
                theme::COMMENT_PAGE
            } else {
                theme::NORMAL
            };

            ListItem::new(Line::from(Span::styled(text, style)))
        })
        .collect();

    let title = if app.visual_anchor.is_some() {
        " Visual "
    } else {
        " Pages "
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::RIGHT)
        .border_style(theme::BORDER);

    let list = List::new(items).block(block);

    let mut state = ListState::default().with_selected(Some(app.current_page()));
    f.render_stateful_widget(list, area, &mut state);
    app.sidebar_offset = state.offset();
}

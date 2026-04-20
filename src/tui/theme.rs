use ratatui::style::{Color, Modifier, Style};

pub const SELECTED: Style = Style::new().fg(Color::Black).bg(Color::Cyan);
pub const DELETED: Style = Style::new()
    .fg(Color::DarkGray)
    .add_modifier(Modifier::CROSSED_OUT);
pub const NORMAL: Style = Style::new().fg(Color::White);
pub const HEADER: Style = Style::new().fg(Color::White).bg(Color::DarkGray);
pub const STATUS: Style = Style::new().fg(Color::White).bg(Color::DarkGray);
pub const HELP_KEY: Style = Style::new().fg(Color::Yellow);
pub const BORDER: Style = Style::new().fg(Color::DarkGray);
pub const SEARCH_MATCH_PAGE: Style = Style::new().fg(Color::Rgb(180, 180, 80));
pub const SEARCH_HIGHLIGHT: Style = Style::new().fg(Color::Black).bg(Color::Yellow);

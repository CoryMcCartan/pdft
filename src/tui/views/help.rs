use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

const BINDINGS: &[(&str, &str)] = &[
    ("j/↓", "Next page"),
    ("k/↑", "Previous page"),
    ("h/←", "Scroll up (text view)"),
    ("l/→", "Scroll down (text view)"),
    ("gg/G", "First/last page"),
    (":", "Go to page number"),
    ("v", "Visual select mode"),
    ("d/x", "Toggle delete (clears group)"),
    ("a+key", "Toggle group assign (e.g. ab)"),
    ("u", "Undo last action"),
    ("s", "Save"),
    ("m", "Merge another PDF"),
    ("/", "Search text"),
    ("n/N", "Next/previous match"),
    ("t", "Toggle image/text view"),
    ("w", "Cycle layout"),
    ("?", "Toggle help"),
    ("q/Esc", "Quit / exit visual mode"),
];

pub fn render(f: &mut Frame, area: Rect) {
    // Full-screen overlay
    f.render_widget(Clear, area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "  Key Bindings",
        theme::STATUS,
    ));
    lines.push(Line::raw(""));

    for (key, desc) in BINDINGS {
        lines.push(Line::from(vec![
            Span::styled(format!("    {key:<8}"), theme::HELP_KEY),
            Span::raw(format!("  {desc}")),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  Press any key to close",
        theme::BORDER,
    )));

    f.render_widget(Paragraph::new(lines), area);
}

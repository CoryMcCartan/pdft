use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// State for a text input dialog.
pub struct InputDialog {
    pub title: String,
    pub prompt: String,
    pub input: String,
    pub cursor: usize,
    pub active: bool,
}

impl InputDialog {
    pub fn new(title: &str, prompt: &str) -> Self {
        Self {
            title: title.to_string(),
            prompt: prompt.to_string(),
            input: String::new(),
            cursor: 0,
            active: false,
        }
    }

    pub fn open(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.active = true;
    }

    /// Open with pre-filled input text.
    pub fn open_with(&mut self, prefill: &str) {
        self.input = prefill.to_string();
        self.cursor = self.input.len();
        self.active = true;
    }

    pub fn close(&mut self) {
        self.active = false;
    }

    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.input[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    pub fn take_input(&mut self) -> String {
        self.active = false;
        std::mem::take(&mut self.input)
    }
}

/// Render the dialog as a bottom command line (like vim's `:` prompt).
/// `area` should be the 1-row hints bar at the bottom of the screen.
pub fn render(f: &mut Frame, area: Rect, dialog: &InputDialog) {
    if !dialog.active {
        return;
    }

    let prompt_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let input_style = Style::default().fg(Color::White);
    let cursor_style = Style::default()
        .fg(Color::Black)
        .bg(Color::White);

    // Available width for input text
    let prompt_text = format!("{} ", dialog.prompt);
    let prompt_len = prompt_text.len();
    let available = (area.width as usize).saturating_sub(prompt_len + 1);

    // Cursor position in characters
    let cursor_char_pos = dialog.input[..dialog.cursor].chars().count();

    // Scroll so cursor is visible
    let scroll = if cursor_char_pos >= available {
        cursor_char_pos - available + 1
    } else {
        0
    };

    // Split input into before-cursor and at-cursor and after-cursor
    let chars: Vec<char> = dialog.input.chars().collect();
    let visible_before: String = chars[scroll..cursor_char_pos].iter().collect();
    let cursor_ch = chars.get(cursor_char_pos).copied().unwrap_or(' ');
    let visible_after: String = if cursor_char_pos + 1 < chars.len() {
        chars[cursor_char_pos + 1..]
            .iter()
            .take(available.saturating_sub(visible_before.len() + 1))
            .collect()
    } else {
        String::new()
    };

    let line = Line::from(vec![
        Span::styled(prompt_text, prompt_style),
        Span::styled(visible_before, input_style),
        Span::styled(cursor_ch.to_string(), cursor_style),
        Span::styled(visible_after, input_style),
    ]);

    f.render_widget(Paragraph::new(line), area);
}

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Widget,
};

/// Single-line text input with cursor, optional masking, and validation error display.
#[derive(Debug, Clone)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
    pub label: String,
    pub masked: bool,
    pub error: Option<String>,
}

impl TextInput {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            value: String::new(),
            cursor: 0,
            label: label.into(),
            masked: false,
            error: None,
        }
    }

    pub fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        let v: String = value.into();
        self.cursor = v.len();
        self.value = v;
        self
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.error = None;
        match key.code {
            KeyCode::Char(c) => {
                self.value.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.value.remove(self.cursor);
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.value.len() {
                    self.value.remove(self.cursor);
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
            }
            KeyCode::Right => {
                if self.cursor < self.value.len() {
                    self.cursor += 1;
                }
            }
            KeyCode::Home => {
                self.cursor = 0;
            }
            KeyCode::End => {
                self.cursor = self.value.len();
            }
            _ => {}
        }
    }

    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.error = Some(msg.into());
    }

    fn display_value(&self) -> String {
        if self.masked {
            "*".repeat(self.value.len())
        } else {
            self.value.clone()
        }
    }
}

/// Renders the text input widget. `focused` controls whether the cursor is shown.
pub struct TextInputWidget<'a> {
    pub input: &'a TextInput,
    pub focused: bool,
}

impl<'a> TextInputWidget<'a> {
    pub fn new(input: &'a TextInput, focused: bool) -> Self {
        Self { input, focused }
    }
}

impl Widget for TextInputWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        // Line 0: label
        let label_line = Line::from(Span::styled(
            &self.input.label,
            Style::default().fg(Color::Cyan),
        ));
        let label_area = Rect { height: 1, ..area };
        label_line.render(label_area, buf);

        // Line 1: input field
        if area.height > 1 {
            let display = self.input.display_value();
            let input_area = Rect {
                y: area.y + 1,
                height: 1,
                ..area
            };

            let prefix = "> ";
            let line = Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::DarkGray)),
                Span::raw(&display),
            ]);
            line.render(input_area, buf);

            // Show cursor
            if self.focused {
                let cursor_x = input_area.x + prefix.len() as u16 + self.input.cursor as u16;
                if cursor_x < input_area.x + input_area.width {
                    let cursor_y = input_area.y;
                    if let Some(cell) = buf.cell_mut((cursor_x, cursor_y)) {
                        cell.set_style(Style::default().bg(Color::White).fg(Color::Black));
                    }
                }
            }
        }

        // Line 2: error message (if any)
        if area.height > 2 {
            if let Some(ref err) = self.input.error {
                let err_area = Rect {
                    y: area.y + 2,
                    height: 1,
                    ..area
                };
                let err_line = Line::from(Span::styled(
                    err,
                    Style::default().fg(Color::Red),
                ));
                err_line.render(err_area, buf);
            }
        }
    }
}

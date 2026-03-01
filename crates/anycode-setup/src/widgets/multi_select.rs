use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

/// Checkbox multi-select list with space to toggle.
#[derive(Debug, Clone)]
pub struct MultiSelect {
    pub items: Vec<String>,
    pub checked: Vec<bool>,
    pub cursor: usize,
    pub label: String,
    pub error: Option<String>,
}

impl MultiSelect {
    pub fn new(label: impl Into<String>, items: Vec<String>) -> Self {
        let len = items.len();
        Self {
            items,
            checked: vec![false; len],
            cursor: 0,
            label: label.into(),
            error: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.error = None;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.cursor + 1 < self.items.len() {
                    self.cursor += 1;
                }
            }
            KeyCode::Char(' ') => {
                self.checked[self.cursor] = !self.checked[self.cursor];
            }
            _ => {}
        }
    }

    /// Returns indices of selected items.
    #[allow(dead_code)]
    pub fn selected_indices(&self) -> Vec<usize> {
        self.checked
            .iter()
            .enumerate()
            .filter(|(_, &c)| c)
            .map(|(i, _)| i)
            .collect()
    }

    pub fn any_selected(&self) -> bool {
        self.checked.iter().any(|&c| c)
    }

    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.error = Some(msg.into());
    }
}

pub struct MultiSelectWidget<'a> {
    pub list: &'a MultiSelect,
}

impl<'a> MultiSelectWidget<'a> {
    pub fn new(list: &'a MultiSelect) -> Self {
        Self { list }
    }
}

impl Widget for MultiSelectWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        // Label
        let label_line = Line::from(Span::styled(
            &self.list.label,
            Style::default().fg(Color::Cyan),
        ));
        let label_area = Rect { height: 1, ..area };
        label_line.render(label_area, buf);

        // Items
        for (i, item) in self.list.items.iter().enumerate() {
            let row = i as u16 + 1;
            if row >= area.height {
                break;
            }
            let is_cursor = i == self.list.cursor;
            let is_checked = self.list.checked[i];
            let check = if is_checked { "[x]" } else { "[ ]" };
            let marker = if is_cursor { ">" } else { " " };
            let style = if is_cursor {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if is_checked {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };
            let line = Line::from(Span::styled(format!(" {marker} {check} {item}"), style));
            let item_area = Rect {
                y: area.y + row,
                height: 1,
                ..area
            };
            line.render(item_area, buf);
        }

        // Hint
        let hint_row = self.list.items.len() as u16 + 1;
        if hint_row < area.height {
            let hint_area = Rect {
                y: area.y + hint_row,
                height: 1,
                ..area
            };
            let hint = Line::from(Span::styled(
                "  [Space] toggle  [Enter] confirm",
                Style::default().fg(Color::DarkGray),
            ));
            hint.render(hint_area, buf);
        }

        // Error
        if let Some(ref err) = self.list.error {
            let err_row = self.list.items.len() as u16 + 2;
            if err_row < area.height {
                let err_area = Rect {
                    y: area.y + err_row,
                    height: 1,
                    ..area
                };
                let err_line = Line::from(Span::styled(err.as_str(), Style::default().fg(Color::Red)));
                err_line.render(err_area, buf);
            }
        }
    }
}

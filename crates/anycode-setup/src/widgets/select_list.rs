use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

/// Arrow-key / j/k single-select list.
#[derive(Debug, Clone)]
pub struct SelectList {
    pub items: Vec<String>,
    pub selected: usize,
    pub label: String,
}

impl SelectList {
    pub fn new(label: impl Into<String>, items: Vec<String>) -> Self {
        Self {
            items,
            selected: 0,
            label: label.into(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.items.len() {
                    self.selected += 1;
                }
            }
            _ => {}
        }
    }

    pub fn selected_value(&self) -> &str {
        &self.items[self.selected]
    }

    /// Set the selected index to the item matching `value`, if found.
    pub fn select_value(&mut self, value: &str) {
        if let Some(idx) = self.items.iter().position(|i| i == value) {
            self.selected = idx;
        }
    }
}

pub struct SelectListWidget<'a> {
    pub list: &'a SelectList,
}

impl<'a> SelectListWidget<'a> {
    pub fn new(list: &'a SelectList) -> Self {
        Self { list }
    }
}

impl Widget for SelectListWidget<'_> {
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
            let is_selected = i == self.list.selected;
            let marker = if is_selected { ">" } else { " " };
            let style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let line = Line::from(Span::styled(format!(" {marker} {item}"), style));
            let item_area = Rect {
                y: area.y + row,
                height: 1,
                ..area
            };
            line.render(item_area, buf);
        }
    }
}

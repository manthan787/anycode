use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};
use crate::data::WizardData;
use super::{Step, StepAction};

pub struct DoneStep;

impl DoneStep {
    pub fn new() -> Self {
        Self
    }
}

impl Step for DoneStep {
    fn handle_key(&mut self, key: KeyEvent, _data: &mut WizardData) -> StepAction {
        match key.code {
            KeyCode::Char('r') => {
                // We'll signal that we want to run. The app loop handles the actual exec.
                StepAction::Quit // Return quit; app will check if user wanted to run
            }
            KeyCode::Char('q') | KeyCode::Esc => StepAction::Quit,
            _ => StepAction::Nothing,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, _data: &WizardData) {
        let cmd = "./target/release/anycode --config config.toml";
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Setup Complete!",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Your configuration has been written to config.toml",
                Style::default().fg(Color::White),
            )),
            Line::from("  and the project has been built."),
            Line::from(""),
            Line::from(Span::styled(
                "  To start anycode, run:",
                Style::default().fg(Color::Gray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("    {cmd}"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "  [r] Run now  |  [q] Quit",
                Style::default().fg(Color::Cyan),
            )),
        ];

        let text = ratatui::widgets::Paragraph::new(lines);
        frame.render_widget(text, area);
    }
}

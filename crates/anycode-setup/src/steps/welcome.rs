use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::data::WizardData;
use super::{Step, StepAction};

pub struct WelcomeStep;

impl WelcomeStep {
    pub fn new() -> Self {
        Self
    }
}

impl Step for WelcomeStep {
    fn handle_key(&mut self, key: KeyEvent, _data: &mut WizardData) -> StepAction {
        match key.code {
            KeyCode::Enter => StepAction::NextStep,
            KeyCode::Esc => StepAction::Quit,
            _ => StepAction::Nothing,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, _data: &WizardData) {
        let lines = vec![
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "   ╔═╗ ╔╗╔ ╦ ╦ ╔═╗ ╔═╗ ╔╦╗ ╔═╗",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "   ╠═╣ ║║║ ╚╦╝ ║   ║ ║  ║║ ║╣ ",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "   ╩ ╩ ╝╚╝  ╩  ╚═╝ ╚═╝ ═╩╝ ╚═╝",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "   Run any coding agent from Telegram or Slack",
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "   This wizard will guide you through:",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                "     1. Checking prerequisites (Rust, Docker)",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                "     2. Configuring messaging platforms",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                "     3. Setting up sandbox environments",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                "     4. Selecting and configuring AI agents",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                "     5. Building and running anycode",
                Style::default().fg(Color::Gray),
            )),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "   Press [Enter] to get started",
                Style::default().fg(Color::Green),
            )),
        ];

        let text = ratatui::widgets::Paragraph::new(lines);
        frame.render_widget(text, area);
    }
}

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::time::Duration;

use crate::data::WizardData;
use crate::steps::{Step, StepAction};
use crate::steps::{
    agents::AgentsStep, build::BuildStep, done::DoneStep, messaging::MessagingStep,
    prerequisites::PrerequisitesStep, review::ReviewStep, sandbox::SandboxStep,
    welcome::WelcomeStep,
};

const STEP_COUNT: usize = 8;

const STEP_LABELS: [&str; STEP_COUNT] = [
    "Welcome",
    "Prerequisites",
    "Messaging",
    "Sandbox",
    "Agents",
    "Review",
    "Build",
    "Done",
];

pub struct App {
    pub data: WizardData,
    pub current_step: usize,
    pub should_quit: bool,
    pub should_run: bool,
    steps: Vec<Box<dyn Step>>,
}

impl App {
    pub fn new() -> Self {
        let steps: Vec<Box<dyn Step>> = vec![
            Box::new(WelcomeStep::new()),
            Box::new(PrerequisitesStep::new()),
            Box::new(MessagingStep::new()),
            Box::new(SandboxStep::new()),
            Box::new(AgentsStep::new()),
            Box::new(ReviewStep::new()),
            Box::new(BuildStep::new()),
            Box::new(DoneStep::new()),
        ];
        Self {
            data: WizardData::default(),
            current_step: 0,
            should_quit: false,
            should_run: false,
            steps,
        }
    }

    /// Run the main event loop. Returns Ok(true) if user chose to run the daemon.
    pub fn run<B: ratatui::backend::Backend<Error: Send + Sync + 'static>>(
        &mut self,
        terminal: &mut ratatui::Terminal<B>,
    ) -> anyhow::Result<bool> {
        // Enter the first step
        self.steps[0].on_enter(&self.data);

        loop {
            terminal.draw(|frame| self.render(frame))?;

            if self.should_quit {
                return Ok(self.should_run);
            }

            // Tick the current step (no-op for most steps; BuildStep polls subprocess)
            self.steps[self.current_step].tick(&self.data);

            // Use a shorter poll timeout during builds to stay responsive
            let timeout = if self.current_step == 6 {
                Duration::from_millis(50)
            } else {
                Duration::from_millis(100)
            };

            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    // Global Ctrl+C handler
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        self.should_quit = true;
                        continue;
                    }

                    let action =
                        self.steps[self.current_step].handle_key(key, &mut self.data);
                    match action {
                        StepAction::Nothing => {}
                        StepAction::NextStep => {
                            if self.current_step + 1 < STEP_COUNT {
                                self.current_step += 1;
                                self.steps[self.current_step].on_enter(&self.data);
                            }
                        }
                        StepAction::PrevStep => {
                            if self.current_step > 0 {
                                self.current_step -= 1;
                                self.steps[self.current_step].on_enter(&self.data);
                            }
                        }
                        StepAction::Quit => {
                            // On the Done step, 'r' triggers quit with should_run
                            if self.current_step == STEP_COUNT - 1
                                && key.code == KeyCode::Char('r')
                            {
                                self.should_run = true;
                            }
                            self.should_quit = true;
                        }
                    }
                }
            }
        }
    }

    fn render(&self, frame: &mut Frame) {
        let outer = frame.area();

        let chunks = Layout::vertical([
            Constraint::Length(3), // progress
            Constraint::Min(10),  // content
            Constraint::Length(1), // footer
        ])
        .split(outer);

        self.render_progress(frame, chunks[0]);
        self.render_content(frame, chunks[1]);
        self.render_footer(frame, chunks[2]);
    }

    fn render_progress(&self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![Span::raw("  ")];
        for (i, _label) in STEP_LABELS.iter().enumerate() {
            let style = if i == self.current_step {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if i < self.current_step {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let num = format!("[{}]", i + 1);
            spans.push(Span::styled(num, style));
            spans.push(Span::raw(" "));
        }

        let line = Line::from(spans);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Anycode Setup ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
        let paragraph = Paragraph::new(line).block(block);
        frame.render_widget(paragraph, area);
    }

    fn render_content(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        self.steps[self.current_step].render(frame, inner, &self.data);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let footer = Line::from(vec![
            Span::styled(" [Enter] ", Style::default().fg(Color::Green)),
            Span::raw("Next  "),
            Span::styled("[Esc] ", Style::default().fg(Color::Yellow)),
            Span::raw("Back  "),
            Span::styled("[Ctrl+C] ", Style::default().fg(Color::Red)),
            Span::raw("Quit"),
        ]);
        let paragraph = Paragraph::new(footer);
        frame.render_widget(paragraph, area);
    }
}

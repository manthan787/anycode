use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::path::Path;
use std::sync::mpsc;

use crate::config_gen::generate_config;
use crate::data::{SandboxProvider, WizardData};
use crate::runner::{self, OutputLine};
use super::{Step, StepAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    WriteConfig,
    ConfirmOverwrite,
    CargoBuild,
    DockerBuild,
    Done,
    Failed,
}

pub struct BuildStep {
    phase: Phase,
    output_lines: Vec<(Color, String)>,
    /// Receives output from running subprocess.
    receiver: Option<mpsc::Receiver<OutputLine>>,
    config_written: bool,
    cargo_ok: bool,
    docker_ok: bool,
    docker_skipped: bool,
    error_message: String,
}

impl BuildStep {
    pub fn new() -> Self {
        Self {
            phase: Phase::WriteConfig,
            output_lines: Vec::new(),
            receiver: None,
            config_written: false,
            cargo_ok: false,
            docker_ok: false,
            docker_skipped: false,
            error_message: String::new(),
        }
    }

    /// Poll the subprocess receiver for new output. Returns true if the process finished.
    pub fn poll(&mut self) -> bool {
        let rx = match self.receiver.as_ref() {
            Some(rx) => rx,
            None => return false,
        };

        loop {
            match rx.try_recv() {
                Ok(OutputLine::Stdout(line)) => {
                    self.output_lines.push((Color::White, line));
                    // Keep output buffer bounded
                    if self.output_lines.len() > 200 {
                        self.output_lines.remove(0);
                    }
                }
                Ok(OutputLine::Stderr(line)) => {
                    self.output_lines.push((Color::Yellow, line));
                    if self.output_lines.len() > 200 {
                        self.output_lines.remove(0);
                    }
                }
                Ok(OutputLine::Finished(code)) => {
                    self.receiver = None;
                    let success = code == Some(0);
                    match self.phase {
                        Phase::CargoBuild => {
                            self.cargo_ok = success;
                            if success {
                                self.output_lines.push((Color::Green, "[OK] cargo build --release".into()));
                            } else {
                                self.output_lines.push((Color::Red, "[FAIL] cargo build --release".into()));
                                self.error_message = "Cargo build failed. Press [r] to retry or [q] to quit.".into();
                                self.phase = Phase::Failed;
                            }
                            return true;
                        }
                        Phase::DockerBuild => {
                            self.docker_ok = success;
                            if success {
                                self.output_lines.push((Color::Green, "[OK] docker build".into()));
                            } else {
                                self.output_lines.push((Color::Yellow, "[WARN] docker build failed (you can rebuild later)".into()));
                            }
                            self.phase = Phase::Done;
                            return true;
                        }
                        _ => return true,
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return false,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.receiver = None;
                    return true;
                }
            }
        }
    }

    fn write_config(&mut self, data: &WizardData) {
        let config = generate_config(data);
        if let Err(e) = std::fs::write("config.toml", &config) {
            self.output_lines.push((Color::Red, format!("[FAIL] writing config.toml: {e}")));
            self.error_message = format!("Failed to write config.toml: {e}");
            self.phase = Phase::Failed;
            return;
        }
        self.config_written = true;
        self.output_lines.push((Color::Green, "[OK] config.toml written".into()));
    }

    fn start_cargo_build(&mut self) {
        self.output_lines.push((Color::Cyan, "[RUNNING] cargo build --release ...".into()));
        self.phase = Phase::CargoBuild;
        match runner::run_command("cargo", &["build", "--release"]) {
            Ok(rx) => self.receiver = Some(rx),
            Err(e) => {
                self.output_lines.push((Color::Red, format!("[FAIL] failed to start cargo: {e}")));
                self.error_message = "Failed to start cargo build.".into();
                self.phase = Phase::Failed;
            }
        }
    }

    fn start_docker_build(&mut self) {
        self.output_lines.push((Color::Cyan, "[RUNNING] docker build ...".into()));
        self.phase = Phase::DockerBuild;
        match runner::run_command(
            "docker",
            &["build", "-f", "docker/Dockerfile.agent", "-t", "anycode-sandbox:latest", "."],
        ) {
            Ok(rx) => self.receiver = Some(rx),
            Err(e) => {
                self.output_lines.push((Color::Yellow, format!("[WARN] failed to start docker build: {e}")));
                self.docker_ok = false;
                self.phase = Phase::Done;
            }
        }
    }
}

impl Step for BuildStep {
    fn on_enter(&mut self, data: &WizardData) {
        if self.config_written {
            return; // Already in progress or done
        }

        // Check if config.toml exists
        if Path::new("config.toml").exists() {
            self.phase = Phase::ConfirmOverwrite;
        } else {
            self.write_config(data);
            if self.phase != Phase::Failed {
                self.start_cargo_build();
            }
        }

        self.docker_skipped = data.sandbox_provider == SandboxProvider::Ecs;
    }

    fn handle_key(&mut self, key: KeyEvent, data: &mut WizardData) -> StepAction {
        match self.phase {
            Phase::ConfirmOverwrite => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.write_config(data);
                    if self.phase != Phase::Failed {
                        self.start_cargo_build();
                    }
                    StepAction::Nothing
                }
                KeyCode::Char('n') | KeyCode::Esc => StepAction::Quit,
                _ => StepAction::Nothing,
            },
            Phase::CargoBuild | Phase::DockerBuild => {
                // Just poll, ignore keys during build
                StepAction::Nothing
            }
            Phase::Done => match key.code {
                KeyCode::Enter => StepAction::NextStep,
                _ => StepAction::Nothing,
            },
            Phase::Failed => match key.code {
                KeyCode::Char('r') => {
                    self.error_message.clear();
                    self.start_cargo_build();
                    StepAction::Nothing
                }
                KeyCode::Char('q') => StepAction::Quit,
                _ => StepAction::Nothing,
            },
            Phase::WriteConfig => StepAction::Nothing,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, _data: &WizardData) {
        let mut lines = vec![
            Line::from(Span::styled(
                "  Build",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];

        if self.phase == Phase::ConfirmOverwrite {
            lines.push(Line::from(Span::styled(
                "  config.toml already exists. Overwrite? [y/n]",
                Style::default().fg(Color::Yellow),
            )));
            let text = Paragraph::new(lines);
            frame.render_widget(text, area);
            return;
        }

        // Show output lines (last N that fit)
        let max_lines = area.height.saturating_sub(5) as usize;
        let start = if self.output_lines.len() > max_lines {
            self.output_lines.len() - max_lines
        } else {
            0
        };

        for (color, text) in &self.output_lines[start..] {
            lines.push(Line::from(Span::styled(
                format!("  {text}"),
                Style::default().fg(*color),
            )));
        }

        // Status / spinner hint
        match self.phase {
            Phase::CargoBuild => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Building... (this may take a while)",
                    Style::default().fg(Color::Cyan),
                )));
            }
            Phase::DockerBuild => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Building Docker image...",
                    Style::default().fg(Color::Cyan),
                )));
            }
            Phase::Done => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Build complete! Press [Enter] to continue.",
                    Style::default().fg(Color::Green),
                )));
            }
            Phase::Failed => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("  {}", self.error_message),
                    Style::default().fg(Color::Red),
                )));
            }
            _ => {}
        }

        let text = Paragraph::new(lines);
        frame.render_widget(text, area);
    }

    fn tick(&mut self, _data: &WizardData) {
        if self.receiver.is_none() {
            return;
        }
        let finished = self.poll();
        if finished && self.phase == Phase::CargoBuild && self.cargo_ok {
            if self.docker_skipped {
                self.output_lines.push((Color::DarkGray, "[SKIP] docker build (ECS provider selected)".into()));
                self.phase = Phase::Done;
            } else {
                self.start_docker_build();
            }
        }
    }
}

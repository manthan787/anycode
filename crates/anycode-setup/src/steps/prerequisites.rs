use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};
use std::process::Command;

use crate::data::WizardData;
use super::{Step, StepAction};

#[derive(Debug, Clone)]
struct CheckResult {
    name: String,
    passed: bool,
    version: String,
    hint: String,
}

pub struct PrerequisitesStep {
    checks: Vec<CheckResult>,
}

impl PrerequisitesStep {
    pub fn new() -> Self {
        let mut step = Self { checks: Vec::new() };
        step.run_checks();
        step
    }

    fn run_checks(&mut self) {
        self.checks.clear();

        // Check rustc
        let rustc = Command::new("rustc").arg("--version").output();
        self.checks.push(match rustc {
            Ok(out) if out.status.success() => CheckResult {
                name: "rustc".into(),
                passed: true,
                version: String::from_utf8_lossy(&out.stdout).trim().into(),
                hint: String::new(),
            },
            _ => CheckResult {
                name: "rustc".into(),
                passed: false,
                version: "not found".into(),
                hint: "Install Rust: https://rustup.rs".into(),
            },
        });

        // Check cargo
        let cargo = Command::new("cargo").arg("--version").output();
        self.checks.push(match cargo {
            Ok(out) if out.status.success() => CheckResult {
                name: "cargo".into(),
                passed: true,
                version: String::from_utf8_lossy(&out.stdout).trim().into(),
                hint: String::new(),
            },
            _ => CheckResult {
                name: "cargo".into(),
                passed: false,
                version: "not found".into(),
                hint: "Install Rust: https://rustup.rs".into(),
            },
        });

        // Check docker
        let docker = Command::new("docker").arg("info").output();
        self.checks.push(match docker {
            Ok(out) if out.status.success() => {
                let ver = Command::new("docker")
                    .arg("--version")
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_else(|_| "installed".into());
                CheckResult {
                    name: "docker".into(),
                    passed: true,
                    version: ver,
                    hint: String::new(),
                }
            }
            _ => CheckResult {
                name: "docker".into(),
                passed: false,
                version: "not running".into(),
                hint: "Install Docker: https://docs.docker.com/get-docker/".into(),
            },
        });
    }

    fn all_pass(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }
}

impl Step for PrerequisitesStep {
    fn handle_key(&mut self, key: KeyEvent, _data: &mut WizardData) -> StepAction {
        match key.code {
            KeyCode::Enter if self.all_pass() => StepAction::NextStep,
            KeyCode::Char('r') => {
                self.run_checks();
                StepAction::Nothing
            }
            KeyCode::Esc => StepAction::PrevStep,
            _ => StepAction::Nothing,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, _data: &WizardData) {
        let mut lines = vec![
            Line::from(Span::styled(
                "  Checking prerequisites...",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];

        for check in &self.checks {
            let (icon, color) = if check.passed {
                ("  [OK]", Color::Green)
            } else {
                ("  [X] ", Color::Red)
            };
            lines.push(Line::from(vec![
                Span::styled(icon, Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::raw(format!(" {} - {}", check.name, check.version)),
            ]));
            if !check.hint.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("       {}", check.hint),
                    Style::default().fg(Color::Yellow),
                )));
            }
        }

        lines.push(Line::from(""));
        if self.all_pass() {
            lines.push(Line::from(Span::styled(
                "  All prerequisites met! Press [Enter] to continue.",
                Style::default().fg(Color::Green),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "  Some prerequisites are missing. Press [r] to retry.",
                Style::default().fg(Color::Yellow),
            )));
        }

        let text = ratatui::widgets::Paragraph::new(lines);
        frame.render_widget(text, area);
    }
}

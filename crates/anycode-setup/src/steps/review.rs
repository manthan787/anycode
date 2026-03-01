use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::config_gen::mask_secret;
use crate::data::{SandboxProvider, WizardData};
use super::{Step, StepAction};

pub struct ReviewStep;

impl ReviewStep {
    pub fn new() -> Self {
        Self
    }
}

impl Step for ReviewStep {
    fn handle_key(&mut self, key: KeyEvent, _data: &mut WizardData) -> StepAction {
        match key.code {
            KeyCode::Enter => StepAction::NextStep,
            KeyCode::Esc => StepAction::PrevStep,
            _ => StepAction::Nothing,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, data: &WizardData) {
        let mut lines: Vec<Line<'static>> = vec![
            Line::from(Span::styled(
                "  Configuration Summary",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];

        // Messaging
        lines.push(section_header("Messaging"));
        if data.enable_telegram {
            lines.push(kv("Telegram token", &mask_secret(&data.telegram_bot_token)));
            if !data.telegram_allowed_users.is_empty() {
                lines.push(kv("Allowed users", &data.telegram_allowed_users));
            }
        }
        if data.enable_slack {
            lines.push(kv("Slack app token", &mask_secret(&data.slack_app_token)));
            lines.push(kv("Slack bot token", &mask_secret(&data.slack_bot_token)));
            if !data.slack_allowed_users.is_empty() {
                lines.push(kv("Allowed users", &data.slack_allowed_users));
            }
        }
        lines.push(Line::from(""));

        // Sandbox
        lines.push(section_header("Sandbox"));
        match data.sandbox_provider {
            SandboxProvider::Docker => {
                lines.push(kv("Provider", "docker"));
                lines.push(kv("Image", &data.docker_image));
                let ports = format!("{}-{}", data.docker_port_start, data.docker_port_end);
                lines.push(kv("Ports", &ports));
                lines.push(kv("Network", &data.docker_network));
            }
            SandboxProvider::Ecs => {
                lines.push(kv("Provider", "ecs"));
                lines.push(kv("Cluster", &data.ecs_cluster));
                lines.push(kv("Task def", &data.ecs_task_definition));
                lines.push(kv("Subnets", &data.ecs_subnets));
                if !data.ecs_security_groups.is_empty() {
                    lines.push(kv("Security groups", &data.ecs_security_groups));
                }
                if !data.ecs_region.is_empty() {
                    lines.push(kv("Region", &data.ecs_region));
                }
            }
        }
        lines.push(Line::from(""));

        // Agents
        lines.push(section_header("Agents"));
        let agents_str = data.selected_agents().join(", ");
        lines.push(kv("Enabled", &agents_str));
        lines.push(kv("Default", &data.default_agent));
        if data.needs_anthropic_key() {
            let masked = mask_secret(&data.anthropic_api_key);
            lines.push(kv("ANTHROPIC_API_KEY", &masked));
        }
        if data.needs_openai_key() {
            let masked = mask_secret(&data.openai_api_key);
            lines.push(kv("OPENAI_API_KEY", &masked));
        }
        lines.push(Line::from(""));

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Press [Enter] to build, [Esc] to go back and edit",
            Style::default().fg(Color::Green),
        )));

        let text = ratatui::widgets::Paragraph::new(lines);
        frame.render_widget(text, area);
    }
}

fn section_header(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  --- {title} ---"),
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    ))
}

fn kv(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("    {key}: "), Style::default().fg(Color::Gray)),
        Span::styled(value.to_string(), Style::default().fg(Color::Yellow)),
    ])
}

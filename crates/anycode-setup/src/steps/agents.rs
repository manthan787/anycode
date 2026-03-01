use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::data::WizardData;
use crate::widgets::{
    MultiSelect, MultiSelectWidget, SelectList, SelectListWidget, TextInput, TextInputWidget,
};
use super::{Step, StepAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubState {
    AgentSelect,
    DefaultAgent,
    AnthropicKey,
    OpenaiKey,
    GitHubToken,
}

pub struct AgentsStep {
    sub_state: SubState,
    agent_select: MultiSelect,
    default_agent: SelectList,
    anthropic_key: TextInput,
    openai_key: TextInput,
    github_token: TextInput,
}

impl AgentsStep {
    pub fn new() -> Self {
        Self {
            sub_state: SubState::AgentSelect,
            agent_select: MultiSelect::new(
                "Select AI agents to configure (at least one):",
                vec!["claude-code".into(), "codex".into(), "goose".into()],
            ),
            default_agent: SelectList::new("Select default agent:", vec![]),
            anthropic_key: TextInput::new("ANTHROPIC_API_KEY:").masked(),
            openai_key: TextInput::new("OPENAI_API_KEY:").masked(),
            github_token: TextInput::new("GitHub Token (optional, for repo access):").masked(),
        }
    }

    fn save_to_data(&self, data: &mut WizardData) {
        data.enable_claude_code = self.agent_select.checked[0];
        data.enable_codex = self.agent_select.checked[1];
        data.enable_goose = self.agent_select.checked[2];
        if !self.default_agent.items.is_empty() {
            data.default_agent = self.default_agent.selected_value().to_string();
        }
        data.anthropic_api_key = self.anthropic_key.value.clone();
        data.openai_api_key = self.openai_key.value.clone();
        data.github_token = self.github_token.value.clone();
    }

    fn load_from_data(&mut self, data: &WizardData) {
        self.agent_select.checked[0] = data.enable_claude_code;
        self.agent_select.checked[1] = data.enable_codex;
        self.agent_select.checked[2] = data.enable_goose;
        self.anthropic_key = TextInput::new("ANTHROPIC_API_KEY:").masked().with_value(&data.anthropic_api_key);
        self.openai_key = TextInput::new("OPENAI_API_KEY:").masked().with_value(&data.openai_api_key);
        self.github_token = TextInput::new("GitHub Token (optional, for repo access):").masked().with_value(&data.github_token);
        self.rebuild_default_list(data);
    }

    fn rebuild_default_list(&mut self, data: &WizardData) {
        let agents = data.selected_agents().iter().map(|s| s.to_string()).collect::<Vec<_>>();
        self.default_agent = SelectList::new("Select default agent:", agents);
        self.default_agent.select_value(&data.default_agent);
    }

    /// Determine the next sub-state after agent selection.
    fn next_after_default(&self) -> SubState {
        if self.agent_select.checked[0] {
            SubState::AnthropicKey
        } else if self.agent_select.checked[1] || self.agent_select.checked[2] {
            SubState::OpenaiKey
        } else {
            // Shouldn't happen since we validate at least one
            SubState::AgentSelect
        }
    }
}

impl Step for AgentsStep {
    fn on_enter(&mut self, data: &WizardData) {
        self.load_from_data(data);
        if !data.enable_claude_code && !data.enable_codex && !data.enable_goose {
            self.sub_state = SubState::AgentSelect;
        }
    }

    fn handle_key(&mut self, key: KeyEvent, data: &mut WizardData) -> StepAction {
        match self.sub_state {
            SubState::AgentSelect => match key.code {
                KeyCode::Enter => {
                    if !self.agent_select.any_selected() {
                        self.agent_select.set_error("Select at least one agent");
                        return StepAction::Nothing;
                    }
                    // Save current selection to data so we can build the default list
                    data.enable_claude_code = self.agent_select.checked[0];
                    data.enable_codex = self.agent_select.checked[1];
                    data.enable_goose = self.agent_select.checked[2];
                    self.rebuild_default_list(data);
                    self.sub_state = SubState::DefaultAgent;
                    StepAction::Nothing
                }
                KeyCode::Esc => StepAction::PrevStep,
                _ => {
                    self.agent_select.handle_key(key);
                    StepAction::Nothing
                }
            },
            SubState::DefaultAgent => match key.code {
                KeyCode::Enter => {
                    data.default_agent = self.default_agent.selected_value().to_string();
                    self.sub_state = self.next_after_default();
                    StepAction::Nothing
                }
                KeyCode::Esc => {
                    self.sub_state = SubState::AgentSelect;
                    StepAction::Nothing
                }
                _ => {
                    self.default_agent.handle_key(key);
                    StepAction::Nothing
                }
            },
            SubState::AnthropicKey => match key.code {
                KeyCode::Enter => {
                    if self.anthropic_key.value.is_empty() {
                        self.anthropic_key.set_error("API key is required for claude-code");
                        return StepAction::Nothing;
                    }
                    self.save_to_data(data);
                    if data.needs_openai_key() {
                        self.sub_state = SubState::OpenaiKey;
                    } else {
                        self.sub_state = SubState::GitHubToken;
                    }
                    StepAction::Nothing
                }
                KeyCode::Esc => {
                    self.sub_state = SubState::DefaultAgent;
                    StepAction::Nothing
                }
                _ => {
                    self.anthropic_key.handle_key(key);
                    StepAction::Nothing
                }
            },
            SubState::OpenaiKey => match key.code {
                KeyCode::Enter => {
                    if self.openai_key.value.is_empty() {
                        self.openai_key.set_error("API key is required for codex/goose");
                        return StepAction::Nothing;
                    }
                    self.save_to_data(data);
                    self.sub_state = SubState::GitHubToken;
                    StepAction::Nothing
                }
                KeyCode::Esc => {
                    if data.needs_anthropic_key() {
                        self.sub_state = SubState::AnthropicKey;
                    } else {
                        self.sub_state = SubState::DefaultAgent;
                    }
                    StepAction::Nothing
                }
                _ => {
                    self.openai_key.handle_key(key);
                    StepAction::Nothing
                }
            },
            SubState::GitHubToken => match key.code {
                KeyCode::Enter => {
                    self.save_to_data(data);
                    StepAction::NextStep
                }
                KeyCode::Esc => {
                    if data.needs_openai_key() {
                        self.sub_state = SubState::OpenaiKey;
                    } else if data.needs_anthropic_key() {
                        self.sub_state = SubState::AnthropicKey;
                    } else {
                        self.sub_state = SubState::DefaultAgent;
                    }
                    StepAction::Nothing
                }
                _ => {
                    self.github_token.handle_key(key);
                    StepAction::Nothing
                }
            },
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, _data: &WizardData) {
        let header_lines = vec![
            Line::from(Span::styled(
                "  Agent Configuration",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];
        let header = ratatui::widgets::Paragraph::new(header_lines);
        frame.render_widget(header, area);

        let content_area = Rect {
            x: area.x + 4,
            y: area.y + 2,
            width: area.width.saturating_sub(8),
            height: area.height.saturating_sub(2),
        };

        match self.sub_state {
            SubState::AgentSelect => {
                frame.render_widget(MultiSelectWidget::new(&self.agent_select), content_area);
            }
            SubState::DefaultAgent => {
                frame.render_widget(SelectListWidget::new(&self.default_agent), content_area);
            }
            SubState::AnthropicKey => {
                frame.render_widget(TextInputWidget::new(&self.anthropic_key, true), content_area);
            }
            SubState::OpenaiKey => {
                let note_lines = vec![
                    Line::from(Span::styled(
                        "Shared across codex and goose",
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(""),
                ];
                let note = ratatui::widgets::Paragraph::new(note_lines);
                frame.render_widget(note, content_area);

                let input_area = Rect {
                    y: content_area.y + 2,
                    height: content_area.height.saturating_sub(2),
                    ..content_area
                };
                frame.render_widget(TextInputWidget::new(&self.openai_key, true), input_area);
            }
            SubState::GitHubToken => {
                let note_lines = vec![
                    Line::from(Span::styled(
                        "Enables gh CLI and git HTTPS auth inside sandboxes (press Enter to skip)",
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(""),
                ];
                let note = ratatui::widgets::Paragraph::new(note_lines);
                frame.render_widget(note, content_area);

                let input_area = Rect {
                    y: content_area.y + 2,
                    height: content_area.height.saturating_sub(2),
                    ..content_area
                };
                frame.render_widget(TextInputWidget::new(&self.github_token, true), input_area);
            }
        }
    }
}

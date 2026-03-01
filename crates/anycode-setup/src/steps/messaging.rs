use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::data::WizardData;
use crate::widgets::{MultiSelect, MultiSelectWidget, TextInput, TextInputWidget};
use super::{Step, StepAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubState {
    PlatformSelect,
    TelegramToken,
    TelegramUsers,
    SlackAppToken,
    SlackBotToken,
    SlackUsers,
}

pub struct MessagingStep {
    sub_state: SubState,
    platform_select: MultiSelect,
    telegram_token: TextInput,
    telegram_users: TextInput,
    slack_app_token: TextInput,
    slack_bot_token: TextInput,
    slack_users: TextInput,
}

impl MessagingStep {
    pub fn new() -> Self {
        Self {
            sub_state: SubState::PlatformSelect,
            platform_select: MultiSelect::new(
                "Select messaging platforms (at least one):",
                vec!["Telegram".into(), "Slack".into()],
            ),
            telegram_token: TextInput::new("Telegram Bot Token:").masked(),
            telegram_users: TextInput::new("Allowed user IDs (comma-separated, leave empty for all):"),
            slack_app_token: TextInput::new("Slack App Token (xapp-...):").masked(),
            slack_bot_token: TextInput::new("Slack Bot Token (xoxb-...):").masked(),
            slack_users: TextInput::new("Allowed Slack user IDs (comma-separated, leave empty for all):"),
        }
    }

    fn telegram_selected(&self) -> bool {
        self.platform_select.checked[0]
    }

    fn slack_selected(&self) -> bool {
        self.platform_select.checked[1]
    }

    /// Determine the next sub-state after the current one, given selected platforms.
    fn next_sub_state(&self) -> Option<SubState> {
        match self.sub_state {
            SubState::PlatformSelect => {
                if self.telegram_selected() {
                    Some(SubState::TelegramToken)
                } else if self.slack_selected() {
                    Some(SubState::SlackAppToken)
                } else {
                    None
                }
            }
            SubState::TelegramToken => Some(SubState::TelegramUsers),
            SubState::TelegramUsers => {
                if self.slack_selected() {
                    Some(SubState::SlackAppToken)
                } else {
                    None // done
                }
            }
            SubState::SlackAppToken => Some(SubState::SlackBotToken),
            SubState::SlackBotToken => Some(SubState::SlackUsers),
            SubState::SlackUsers => None, // done
        }
    }

    /// Determine the previous sub-state.
    fn prev_sub_state(&self) -> Option<SubState> {
        match self.sub_state {
            SubState::PlatformSelect => None,
            SubState::TelegramToken => Some(SubState::PlatformSelect),
            SubState::TelegramUsers => Some(SubState::TelegramToken),
            SubState::SlackAppToken => {
                if self.telegram_selected() {
                    Some(SubState::TelegramUsers)
                } else {
                    Some(SubState::PlatformSelect)
                }
            }
            SubState::SlackBotToken => Some(SubState::SlackAppToken),
            SubState::SlackUsers => Some(SubState::SlackBotToken),
        }
    }

    fn save_to_data(&self, data: &mut WizardData) {
        data.enable_telegram = self.telegram_selected();
        data.enable_slack = self.slack_selected();
        data.telegram_bot_token = self.telegram_token.value.clone();
        data.telegram_allowed_users = self.telegram_users.value.clone();
        data.slack_app_token = self.slack_app_token.value.clone();
        data.slack_bot_token = self.slack_bot_token.value.clone();
        data.slack_allowed_users = self.slack_users.value.clone();
    }

    fn load_from_data(&mut self, data: &WizardData) {
        self.platform_select.checked[0] = data.enable_telegram;
        self.platform_select.checked[1] = data.enable_slack;
        self.telegram_token = TextInput::new("Telegram Bot Token:")
            .masked()
            .with_value(&data.telegram_bot_token);
        self.telegram_users = TextInput::new("Allowed user IDs (comma-separated, leave empty for all):")
            .with_value(&data.telegram_allowed_users);
        self.slack_app_token = TextInput::new("Slack App Token (xapp-...):")
            .masked()
            .with_value(&data.slack_app_token);
        self.slack_bot_token = TextInput::new("Slack Bot Token (xoxb-...):")
            .masked()
            .with_value(&data.slack_bot_token);
        self.slack_users = TextInput::new("Allowed Slack user IDs (comma-separated, leave empty for all):")
            .with_value(&data.slack_allowed_users);
    }
}

impl Step for MessagingStep {
    fn on_enter(&mut self, data: &WizardData) {
        self.load_from_data(data);
        // Reset to platform select when entering step fresh
        if !data.enable_telegram && !data.enable_slack {
            self.sub_state = SubState::PlatformSelect;
        }
    }

    fn handle_key(&mut self, key: KeyEvent, data: &mut WizardData) -> StepAction {
        match self.sub_state {
            SubState::PlatformSelect => match key.code {
                KeyCode::Enter => {
                    if !self.platform_select.any_selected() {
                        self.platform_select.set_error("Select at least one platform");
                        return StepAction::Nothing;
                    }
                    self.save_to_data(data);
                    if let Some(next) = self.next_sub_state() {
                        self.sub_state = next;
                    }
                    StepAction::Nothing
                }
                KeyCode::Esc => StepAction::PrevStep,
                _ => {
                    self.platform_select.handle_key(key);
                    StepAction::Nothing
                }
            },
            SubState::TelegramToken => match key.code {
                KeyCode::Enter => {
                    if self.telegram_token.value.is_empty() {
                        self.telegram_token.set_error("Bot token is required");
                        return StepAction::Nothing;
                    }
                    self.save_to_data(data);
                    if let Some(next) = self.next_sub_state() {
                        self.sub_state = next;
                    }
                    StepAction::Nothing
                }
                KeyCode::Esc => {
                    if let Some(prev) = self.prev_sub_state() {
                        self.sub_state = prev;
                    }
                    StepAction::Nothing
                }
                _ => {
                    self.telegram_token.handle_key(key);
                    StepAction::Nothing
                }
            },
            SubState::TelegramUsers => match key.code {
                KeyCode::Enter => {
                    self.save_to_data(data);
                    if let Some(next) = self.next_sub_state() {
                        self.sub_state = next;
                    } else {
                        return StepAction::NextStep;
                    }
                    StepAction::Nothing
                }
                KeyCode::Esc => {
                    if let Some(prev) = self.prev_sub_state() {
                        self.sub_state = prev;
                    }
                    StepAction::Nothing
                }
                _ => {
                    self.telegram_users.handle_key(key);
                    StepAction::Nothing
                }
            },
            SubState::SlackAppToken => match key.code {
                KeyCode::Enter => {
                    if self.slack_app_token.value.is_empty() {
                        self.slack_app_token.set_error("App token is required");
                        return StepAction::Nothing;
                    }
                    self.save_to_data(data);
                    if let Some(next) = self.next_sub_state() {
                        self.sub_state = next;
                    }
                    StepAction::Nothing
                }
                KeyCode::Esc => {
                    if let Some(prev) = self.prev_sub_state() {
                        self.sub_state = prev;
                    }
                    StepAction::Nothing
                }
                _ => {
                    self.slack_app_token.handle_key(key);
                    StepAction::Nothing
                }
            },
            SubState::SlackBotToken => match key.code {
                KeyCode::Enter => {
                    if self.slack_bot_token.value.is_empty() {
                        self.slack_bot_token.set_error("Bot token is required");
                        return StepAction::Nothing;
                    }
                    self.save_to_data(data);
                    if let Some(next) = self.next_sub_state() {
                        self.sub_state = next;
                    }
                    StepAction::Nothing
                }
                KeyCode::Esc => {
                    if let Some(prev) = self.prev_sub_state() {
                        self.sub_state = prev;
                    }
                    StepAction::Nothing
                }
                _ => {
                    self.slack_bot_token.handle_key(key);
                    StepAction::Nothing
                }
            },
            SubState::SlackUsers => match key.code {
                KeyCode::Enter => {
                    self.save_to_data(data);
                    StepAction::NextStep
                }
                KeyCode::Esc => {
                    if let Some(prev) = self.prev_sub_state() {
                        self.sub_state = prev;
                    }
                    StepAction::Nothing
                }
                _ => {
                    self.slack_users.handle_key(key);
                    StepAction::Nothing
                }
            },
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, _data: &WizardData) {
        let mut lines = vec![
            Line::from(Span::styled(
                "  Messaging Platforms",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];

        match self.sub_state {
            SubState::PlatformSelect => {
                // Render the multi-select inline
                let widget_area = Rect {
                    x: area.x + 2,
                    y: area.y + 2,
                    width: area.width.saturating_sub(4),
                    height: area.height.saturating_sub(2),
                };
                let header = ratatui::widgets::Paragraph::new(lines);
                frame.render_widget(header, area);
                frame.render_widget(MultiSelectWidget::new(&self.platform_select), widget_area);
            }
            SubState::TelegramToken => {
                lines.push(Line::from(Span::styled(
                    "  Telegram Configuration",
                    Style::default().fg(Color::Gray),
                )));
                lines.push(Line::from(""));
                let header = ratatui::widgets::Paragraph::new(lines);
                frame.render_widget(header, area);
                let input_area = Rect {
                    x: area.x + 4,
                    y: area.y + 4,
                    width: area.width.saturating_sub(8),
                    height: 3,
                };
                frame.render_widget(TextInputWidget::new(&self.telegram_token, true), input_area);
            }
            SubState::TelegramUsers => {
                lines.push(Line::from(Span::styled(
                    "  Telegram Configuration",
                    Style::default().fg(Color::Gray),
                )));
                lines.push(Line::from(""));
                let header = ratatui::widgets::Paragraph::new(lines);
                frame.render_widget(header, area);
                let input_area = Rect {
                    x: area.x + 4,
                    y: area.y + 4,
                    width: area.width.saturating_sub(8),
                    height: 3,
                };
                frame.render_widget(TextInputWidget::new(&self.telegram_users, true), input_area);
            }
            SubState::SlackAppToken => {
                lines.push(Line::from(Span::styled(
                    "  Slack Configuration",
                    Style::default().fg(Color::Gray),
                )));
                lines.push(Line::from(""));
                let header = ratatui::widgets::Paragraph::new(lines);
                frame.render_widget(header, area);
                let input_area = Rect {
                    x: area.x + 4,
                    y: area.y + 4,
                    width: area.width.saturating_sub(8),
                    height: 3,
                };
                frame.render_widget(TextInputWidget::new(&self.slack_app_token, true), input_area);
            }
            SubState::SlackBotToken => {
                lines.push(Line::from(Span::styled(
                    "  Slack Configuration",
                    Style::default().fg(Color::Gray),
                )));
                lines.push(Line::from(""));
                let header = ratatui::widgets::Paragraph::new(lines);
                frame.render_widget(header, area);
                let input_area = Rect {
                    x: area.x + 4,
                    y: area.y + 4,
                    width: area.width.saturating_sub(8),
                    height: 3,
                };
                frame.render_widget(TextInputWidget::new(&self.slack_bot_token, true), input_area);
            }
            SubState::SlackUsers => {
                lines.push(Line::from(Span::styled(
                    "  Slack Configuration",
                    Style::default().fg(Color::Gray),
                )));
                lines.push(Line::from(""));
                let header = ratatui::widgets::Paragraph::new(lines);
                frame.render_widget(header, area);
                let input_area = Rect {
                    x: area.x + 4,
                    y: area.y + 4,
                    width: area.width.saturating_sub(8),
                    height: 3,
                };
                frame.render_widget(TextInputWidget::new(&self.slack_users, true), input_area);
            }
        }
    }
}

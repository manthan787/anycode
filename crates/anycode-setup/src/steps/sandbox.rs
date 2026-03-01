use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::data::{SandboxProvider, WizardData};
use crate::widgets::{SelectList, SelectListWidget, TextInput, TextInputWidget};
use super::{Step, StepAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubState {
    ProviderSelect,
    DockerImage,
    DockerPortStart,
    DockerPortEnd,
    DockerNetwork,
    EcsCluster,
    EcsTaskDef,
    EcsSubnets,
    EcsSecurityGroups,
    EcsRegion,
}

pub struct SandboxStep {
    sub_state: SubState,
    provider_select: SelectList,
    docker_image: TextInput,
    docker_port_start: TextInput,
    docker_port_end: TextInput,
    docker_network: TextInput,
    ecs_cluster: TextInput,
    ecs_task_def: TextInput,
    ecs_subnets: TextInput,
    ecs_security_groups: TextInput,
    ecs_region: TextInput,
}

impl SandboxStep {
    pub fn new() -> Self {
        Self {
            sub_state: SubState::ProviderSelect,
            provider_select: SelectList::new(
                "Select sandbox provider:",
                vec!["Docker (local)".into(), "ECS (AWS Fargate)".into()],
            ),
            docker_image: TextInput::new("Docker image:").with_value("anycode-sandbox:latest"),
            docker_port_start: TextInput::new("Port range start:").with_value("12000"),
            docker_port_end: TextInput::new("Port range end:").with_value("12100"),
            docker_network: TextInput::new("Docker network:").with_value("bridge"),
            ecs_cluster: TextInput::new("ECS Cluster name (required):"),
            ecs_task_def: TextInput::new("ECS Task Definition (required):"),
            ecs_subnets: TextInput::new("Subnets (comma-separated, required):"),
            ecs_security_groups: TextInput::new("Security groups (comma-separated, optional):"),
            ecs_region: TextInput::new("AWS Region (optional, e.g. us-west-2):"),
        }
    }

    fn is_docker(&self) -> bool {
        self.provider_select.selected == 0
    }

    fn save_to_data(&self, data: &mut WizardData) {
        data.sandbox_provider = if self.is_docker() {
            SandboxProvider::Docker
        } else {
            SandboxProvider::Ecs
        };
        data.docker_image = self.docker_image.value.clone();
        data.docker_port_start = self.docker_port_start.value.clone();
        data.docker_port_end = self.docker_port_end.value.clone();
        data.docker_network = self.docker_network.value.clone();
        data.ecs_cluster = self.ecs_cluster.value.clone();
        data.ecs_task_definition = self.ecs_task_def.value.clone();
        data.ecs_subnets = self.ecs_subnets.value.clone();
        data.ecs_security_groups = self.ecs_security_groups.value.clone();
        data.ecs_region = self.ecs_region.value.clone();
    }

    fn load_from_data(&mut self, data: &WizardData) {
        match data.sandbox_provider {
            SandboxProvider::Docker => self.provider_select.selected = 0,
            SandboxProvider::Ecs => self.provider_select.selected = 1,
        }
        self.docker_image = TextInput::new("Docker image:").with_value(&data.docker_image);
        self.docker_port_start = TextInput::new("Port range start:").with_value(&data.docker_port_start);
        self.docker_port_end = TextInput::new("Port range end:").with_value(&data.docker_port_end);
        self.docker_network = TextInput::new("Docker network:").with_value(&data.docker_network);
        self.ecs_cluster = TextInput::new("ECS Cluster name (required):").with_value(&data.ecs_cluster);
        self.ecs_task_def = TextInput::new("ECS Task Definition (required):").with_value(&data.ecs_task_definition);
        self.ecs_subnets = TextInput::new("Subnets (comma-separated, required):").with_value(&data.ecs_subnets);
        self.ecs_security_groups = TextInput::new("Security groups (comma-separated, optional):").with_value(&data.ecs_security_groups);
        self.ecs_region = TextInput::new("AWS Region (optional, e.g. us-west-2):").with_value(&data.ecs_region);
    }
}

impl Step for SandboxStep {
    fn on_enter(&mut self, data: &WizardData) {
        self.load_from_data(data);
        self.sub_state = SubState::ProviderSelect;
    }

    fn handle_key(&mut self, key: KeyEvent, data: &mut WizardData) -> StepAction {
        match self.sub_state {
            SubState::ProviderSelect => match key.code {
                KeyCode::Enter => {
                    self.save_to_data(data);
                    self.sub_state = if self.is_docker() {
                        SubState::DockerImage
                    } else {
                        SubState::EcsCluster
                    };
                    StepAction::Nothing
                }
                KeyCode::Esc => StepAction::PrevStep,
                _ => {
                    self.provider_select.handle_key(key);
                    StepAction::Nothing
                }
            },
            // Docker flow
            SubState::DockerImage => self.handle_text_input(key, data, &|s| &mut s.docker_image, SubState::DockerPortStart, SubState::ProviderSelect),
            SubState::DockerPortStart => {
                match key.code {
                    KeyCode::Enter => {
                        if self.docker_port_start.value.parse::<u16>().is_err() {
                            self.docker_port_start.set_error("Must be a valid port number");
                            return StepAction::Nothing;
                        }
                        self.save_to_data(data);
                        self.sub_state = SubState::DockerPortEnd;
                        StepAction::Nothing
                    }
                    KeyCode::Esc => {
                        self.sub_state = SubState::DockerImage;
                        StepAction::Nothing
                    }
                    _ => {
                        self.docker_port_start.handle_key(key);
                        StepAction::Nothing
                    }
                }
            }
            SubState::DockerPortEnd => {
                match key.code {
                    KeyCode::Enter => {
                        if self.docker_port_end.value.parse::<u16>().is_err() {
                            self.docker_port_end.set_error("Must be a valid port number");
                            return StepAction::Nothing;
                        }
                        self.save_to_data(data);
                        self.sub_state = SubState::DockerNetwork;
                        StepAction::Nothing
                    }
                    KeyCode::Esc => {
                        self.sub_state = SubState::DockerPortStart;
                        StepAction::Nothing
                    }
                    _ => {
                        self.docker_port_end.handle_key(key);
                        StepAction::Nothing
                    }
                }
            }
            SubState::DockerNetwork => match key.code {
                KeyCode::Enter => {
                    self.save_to_data(data);
                    StepAction::NextStep
                }
                KeyCode::Esc => {
                    self.sub_state = SubState::DockerPortEnd;
                    StepAction::Nothing
                }
                _ => {
                    self.docker_network.handle_key(key);
                    StepAction::Nothing
                }
            },
            // ECS flow
            SubState::EcsCluster => {
                match key.code {
                    KeyCode::Enter => {
                        if self.ecs_cluster.value.trim().is_empty() {
                            self.ecs_cluster.set_error("Cluster name is required");
                            return StepAction::Nothing;
                        }
                        self.save_to_data(data);
                        self.sub_state = SubState::EcsTaskDef;
                        StepAction::Nothing
                    }
                    KeyCode::Esc => {
                        self.sub_state = SubState::ProviderSelect;
                        StepAction::Nothing
                    }
                    _ => {
                        self.ecs_cluster.handle_key(key);
                        StepAction::Nothing
                    }
                }
            }
            SubState::EcsTaskDef => {
                match key.code {
                    KeyCode::Enter => {
                        if self.ecs_task_def.value.trim().is_empty() {
                            self.ecs_task_def.set_error("Task definition is required");
                            return StepAction::Nothing;
                        }
                        self.save_to_data(data);
                        self.sub_state = SubState::EcsSubnets;
                        StepAction::Nothing
                    }
                    KeyCode::Esc => {
                        self.sub_state = SubState::EcsCluster;
                        StepAction::Nothing
                    }
                    _ => {
                        self.ecs_task_def.handle_key(key);
                        StepAction::Nothing
                    }
                }
            }
            SubState::EcsSubnets => {
                match key.code {
                    KeyCode::Enter => {
                        if self.ecs_subnets.value.trim().is_empty() {
                            self.ecs_subnets.set_error("At least one subnet is required");
                            return StepAction::Nothing;
                        }
                        self.save_to_data(data);
                        self.sub_state = SubState::EcsSecurityGroups;
                        StepAction::Nothing
                    }
                    KeyCode::Esc => {
                        self.sub_state = SubState::EcsTaskDef;
                        StepAction::Nothing
                    }
                    _ => {
                        self.ecs_subnets.handle_key(key);
                        StepAction::Nothing
                    }
                }
            }
            SubState::EcsSecurityGroups => self.handle_text_input(key, data, &|s| &mut s.ecs_security_groups, SubState::EcsRegion, SubState::EcsSubnets),
            SubState::EcsRegion => match key.code {
                KeyCode::Enter => {
                    self.save_to_data(data);
                    StepAction::NextStep
                }
                KeyCode::Esc => {
                    self.sub_state = SubState::EcsSecurityGroups;
                    StepAction::Nothing
                }
                _ => {
                    self.ecs_region.handle_key(key);
                    StepAction::Nothing
                }
            },
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, _data: &WizardData) {
        let header_lines = vec![
            Line::from(Span::styled(
                "  Sandbox Configuration",
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
            SubState::ProviderSelect => {
                frame.render_widget(SelectListWidget::new(&self.provider_select), content_area);
            }
            SubState::DockerImage => render_input(frame, content_area, &self.docker_image),
            SubState::DockerPortStart => render_input(frame, content_area, &self.docker_port_start),
            SubState::DockerPortEnd => render_input(frame, content_area, &self.docker_port_end),
            SubState::DockerNetwork => render_input(frame, content_area, &self.docker_network),
            SubState::EcsCluster => render_input(frame, content_area, &self.ecs_cluster),
            SubState::EcsTaskDef => render_input(frame, content_area, &self.ecs_task_def),
            SubState::EcsSubnets => render_input(frame, content_area, &self.ecs_subnets),
            SubState::EcsSecurityGroups => render_input(frame, content_area, &self.ecs_security_groups),
            SubState::EcsRegion => render_input(frame, content_area, &self.ecs_region),
        }
    }
}

impl SandboxStep {
    /// Generic handler for optional text inputs (no validation, just advance/go back).
    fn handle_text_input(
        &mut self,
        key: KeyEvent,
        data: &mut WizardData,
        get_input: &dyn Fn(&mut Self) -> &mut TextInput,
        next: SubState,
        prev: SubState,
    ) -> StepAction {
        match key.code {
            KeyCode::Enter => {
                self.save_to_data(data);
                self.sub_state = next;
                StepAction::Nothing
            }
            KeyCode::Esc => {
                self.sub_state = prev;
                StepAction::Nothing
            }
            _ => {
                get_input(self).handle_key(key);
                StepAction::Nothing
            }
        }
    }
}

fn render_input(frame: &mut Frame, area: Rect, input: &TextInput) {
    frame.render_widget(TextInputWidget::new(input, true), area);
}

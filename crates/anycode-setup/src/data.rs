/// All user input collected across wizard steps.
#[derive(Debug, Clone)]
pub struct WizardData {
    // Messaging
    pub enable_telegram: bool,
    pub enable_slack: bool,
    pub telegram_bot_token: String,
    pub telegram_allowed_users: String,
    pub slack_app_token: String,
    pub slack_bot_token: String,
    pub slack_allowed_users: String,

    // Sandbox
    pub sandbox_provider: SandboxProvider,
    pub docker_image: String,
    pub docker_port_start: String,
    pub docker_port_end: String,
    pub docker_network: String,
    pub ecs_cluster: String,
    pub ecs_task_definition: String,
    pub ecs_subnets: String,
    pub ecs_security_groups: String,
    pub ecs_region: String,

    // Agents
    pub enable_claude_code: bool,
    pub enable_codex: bool,
    pub enable_goose: bool,
    pub default_agent: String,
    pub anthropic_api_key: String,
    pub openai_api_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxProvider {
    Docker,
    Ecs,
}

impl Default for WizardData {
    fn default() -> Self {
        Self {
            enable_telegram: false,
            enable_slack: false,
            telegram_bot_token: String::new(),
            telegram_allowed_users: String::new(),
            slack_app_token: String::new(),
            slack_bot_token: String::new(),
            slack_allowed_users: String::new(),

            sandbox_provider: SandboxProvider::Docker,
            docker_image: "anycode-sandbox:latest".into(),
            docker_port_start: "12000".into(),
            docker_port_end: "12100".into(),
            docker_network: "bridge".into(),
            ecs_cluster: String::new(),
            ecs_task_definition: String::new(),
            ecs_subnets: String::new(),
            ecs_security_groups: String::new(),
            ecs_region: String::new(),

            enable_claude_code: false,
            enable_codex: false,
            enable_goose: false,
            default_agent: "claude-code".into(),
            anthropic_api_key: String::new(),
            openai_api_key: String::new(),
        }
    }
}

impl WizardData {
    /// Returns list of selected agent names.
    pub fn selected_agents(&self) -> Vec<&str> {
        let mut agents = Vec::new();
        if self.enable_claude_code {
            agents.push("claude-code");
        }
        if self.enable_codex {
            agents.push("codex");
        }
        if self.enable_goose {
            agents.push("goose");
        }
        agents
    }

    /// Whether any agent needing OPENAI_API_KEY is selected.
    pub fn needs_openai_key(&self) -> bool {
        self.enable_codex || self.enable_goose
    }

    /// Whether any agent needing ANTHROPIC_API_KEY is selected.
    pub fn needs_anthropic_key(&self) -> bool {
        self.enable_claude_code
    }
}

use serde::Deserialize;
use std::path::Path;

use crate::error::{AnycodeError, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub telegram: Option<TelegramConfig>,
    pub slack: Option<SlackConfig>,
    #[serde(default)]
    pub sandbox: SandboxRuntimeConfig,
    pub docker: DockerConfig,
    #[serde(default)]
    pub ecs: EcsConfig,
    pub database: DatabaseConfig,
    pub agents: AgentsConfig,
    pub github: Option<GitHubConfig>,
    #[serde(default)]
    pub session: SessionConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SandboxRuntimeConfig {
    #[serde(default = "default_sandbox_provider")]
    pub provider: String,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

impl Default for SandboxRuntimeConfig {
    fn default() -> Self {
        Self {
            provider: default_sandbox_provider(),
            protocol: default_protocol(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    #[serde(default)]
    pub allowed_users: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SlackConfig {
    pub app_token: String,
    pub bot_token: String,
    #[serde(default)]
    pub allowed_users: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DockerConfig {
    #[serde(default = "default_image")]
    pub image: String,
    #[serde(default = "default_port_start")]
    pub port_range_start: u16,
    #[serde(default = "default_port_end")]
    pub port_range_end: u16,
    #[serde(default = "default_network")]
    pub network: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EcsConfig {
    #[serde(default)]
    pub cluster: String,
    #[serde(default)]
    pub task_definition: String,
    #[serde(default)]
    pub subnets: Vec<String>,
    #[serde(default)]
    pub security_groups: Vec<String>,
    #[serde(default = "default_assign_public_ip")]
    pub assign_public_ip: bool,
    #[serde(default = "default_container_port")]
    pub container_port: u16,
    #[serde(default = "default_startup_timeout_secs")]
    pub startup_timeout_secs: u64,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub platform_version: Option<String>,
    #[serde(default)]
    pub container_name: Option<String>,
    #[serde(default)]
    pub log_group: Option<String>,
    #[serde(default)]
    pub log_stream_prefix: Option<String>,
}

impl Default for EcsConfig {
    fn default() -> Self {
        Self {
            cluster: String::new(),
            task_definition: String::new(),
            subnets: vec![],
            security_groups: vec![],
            assign_public_ip: default_assign_public_ip(),
            container_port: default_container_port(),
            startup_timeout_secs: default_startup_timeout_secs(),
            poll_interval_ms: default_poll_interval_ms(),
            region: None,
            platform_version: None,
            container_name: None,
            log_group: None,
            log_stream_prefix: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_db_path")]
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentsConfig {
    #[serde(default = "default_agent")]
    pub default_agent: String,
    #[serde(default)]
    pub credentials: std::collections::HashMap<String, AgentCredentials>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentCredentials {
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubConfig {
    pub token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionConfig {
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
    #[serde(default = "default_timeout_minutes")]
    pub timeout_minutes: u64,
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_concurrent: default_max_concurrent(),
            timeout_minutes: default_timeout_minutes(),
            debounce_ms: default_debounce_ms(),
        }
    }
}

fn default_image() -> String {
    "anycode-sandbox:latest".to_string()
}
fn default_sandbox_provider() -> String {
    "docker".to_string()
}
fn default_protocol() -> String {
    "opencode".to_string()
}
fn default_port_start() -> u16 {
    12000
}
fn default_port_end() -> u16 {
    12100
}
fn default_network() -> String {
    "bridge".to_string()
}
fn default_db_path() -> String {
    "anycode.db".to_string()
}
fn default_agent() -> String {
    "claude-code".to_string()
}
fn default_max_concurrent() -> usize {
    5
}
fn default_timeout_minutes() -> u64 {
    30
}
fn default_debounce_ms() -> u64 {
    500
}
fn default_assign_public_ip() -> bool {
    true
}
fn default_container_port() -> u16 {
    2468
}
fn default_startup_timeout_secs() -> u64 {
    120
}
fn default_poll_interval_ms() -> u64 {
    1000
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| AnycodeError::Config(format!("failed to read config file: {e}")))?;
        let config: AppConfig = toml::from_str(&content)
            .map_err(|e| AnycodeError::Config(format!("failed to parse config: {e}")))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.telegram.is_none() && self.slack.is_none() {
            return Err(AnycodeError::Config(
                "at least one messaging platform must be configured (telegram or slack)".into(),
            ));
        }
        if let Some(ref tg) = self.telegram {
            if tg.bot_token.is_empty() {
                return Err(AnycodeError::Config(
                    "telegram.bot_token is required".into(),
                ));
            }
        }
        if let Some(ref slack) = self.slack {
            if slack.app_token.is_empty() {
                return Err(AnycodeError::Config(
                    "slack.app_token is required".into(),
                ));
            }
            if slack.bot_token.is_empty() {
                return Err(AnycodeError::Config(
                    "slack.bot_token is required".into(),
                ));
            }
        }
        match self.sandbox.protocol.as_str() {
            "opencode" | "acpx" => {}
            other => {
                return Err(AnycodeError::Config(format!(
                    "unsupported sandbox.protocol \"{other}\" (expected \"opencode\" or \"acpx\")"
                )));
            }
        }
        if self.sandbox.protocol == "acpx" && self.sandbox.provider != "docker" {
            return Err(AnycodeError::Config(
                "sandbox.protocol = \"acpx\" requires sandbox.provider = \"docker\" (ECS exec not supported yet)".into(),
            ));
        }
        match self.sandbox.provider.as_str() {
            "docker" => {
                if self.docker.port_range_start >= self.docker.port_range_end {
                    return Err(AnycodeError::Config(
                        "docker.port_range_start must be less than port_range_end".into(),
                    ));
                }
            }
            "ecs" => {
                if self.ecs.cluster.trim().is_empty() {
                    return Err(AnycodeError::Config(
                        "ecs.cluster is required when sandbox.provider = \"ecs\"".into(),
                    ));
                }
                if self.ecs.task_definition.trim().is_empty() {
                    return Err(AnycodeError::Config(
                        "ecs.task_definition is required when sandbox.provider = \"ecs\"".into(),
                    ));
                }
                if self.ecs.subnets.is_empty() {
                    return Err(AnycodeError::Config(
                        "ecs.subnets must include at least one subnet when sandbox.provider = \"ecs\""
                            .into(),
                    ));
                }
                if self.ecs.container_port == 0 {
                    return Err(AnycodeError::Config(
                        "ecs.container_port must be greater than 0".into(),
                    ));
                }
            }
            other => {
                return Err(AnycodeError::Config(format!(
                    "unsupported sandbox.provider \"{other}\" (expected \"docker\" or \"ecs\")"
                )));
            }
        }
        Ok(())
    }

    pub fn known_agents(&self) -> Vec<String> {
        let mut agents: Vec<String> = self.agents.credentials.keys().cloned().collect();
        if agents.is_empty() {
            agents.push("claude-code".into());
            agents.push("codex".into());
            agents.push("goose".into());
        }
        agents.sort();
        agents
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_valid_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"
[telegram]
bot_token = "123:ABC"

[docker]
image = "anycode-sandbox:latest"
port_range_start = 12000
port_range_end = 12100

[database]
path = "test.db"

[agents]
default_agent = "claude-code"

[agents.credentials.claude-code]
env = {{ ANTHROPIC_API_KEY = "sk-test" }}
"#
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(config.telegram.as_ref().unwrap().bot_token, "123:ABC");
        assert_eq!(config.docker.port_range_start, 12000);
        assert_eq!(config.agents.default_agent, "claude-code");
    }

    #[test]
    fn test_empty_bot_token_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"
[telegram]
bot_token = ""

[docker]

[database]

[agents]
"#
        )
        .unwrap();

        let err = AppConfig::load(&path).unwrap_err();
        assert!(err.to_string().contains("bot_token"));
    }

    #[test]
    fn test_no_platform_configured_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"
[docker]

[database]

[agents]
"#
        )
        .unwrap();

        let err = AppConfig::load(&path).unwrap_err();
        assert!(err.to_string().contains("at least one messaging platform"));
    }

    #[test]
    fn test_slack_config_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"
[slack]
app_token = "xapp-test"
bot_token = "xoxb-test"
allowed_users = ["U123"]

[docker]

[database]

[agents]
"#
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert!(config.telegram.is_none());
        let slack = config.slack.as_ref().unwrap();
        assert_eq!(slack.app_token, "xapp-test");
        assert_eq!(slack.bot_token, "xoxb-test");
        assert_eq!(slack.allowed_users, vec!["U123".to_string()]);
    }

    #[test]
    fn test_load_ecs_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"
[telegram]
bot_token = "123:ABC"

[sandbox]
provider = "ecs"

[ecs]
cluster = "anycode-cluster"
task_definition = "anycode-task:1"
subnets = ["subnet-123"]
container_port = 2468

[docker]

[database]
path = "test.db"

[agents]
default_agent = "claude-code"
"#
        )
        .unwrap();

        let config = AppConfig::load(&path).unwrap();
        assert_eq!(config.sandbox.provider, "ecs");
        assert_eq!(config.ecs.cluster, "anycode-cluster");
    }

    #[test]
    fn test_ecs_provider_requires_cluster() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"
[telegram]
bot_token = "123:ABC"

[sandbox]
provider = "ecs"

[ecs]
task_definition = "anycode-task:1"
subnets = ["subnet-123"]

[docker]

[database]
path = "test.db"

[agents]
default_agent = "claude-code"
"#
        )
        .unwrap();

        let err = AppConfig::load(&path).unwrap_err();
        assert!(err.to_string().contains("ecs.cluster"));
    }
}

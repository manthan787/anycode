use crate::data::{SandboxProvider, WizardData};

/// Generate a config.toml string from the wizard data.
pub fn generate_config(data: &WizardData) -> String {
    let mut out = String::new();

    // Telegram
    if data.enable_telegram {
        out.push_str("[telegram]\n");
        out.push_str(&format!("bot_token = {}\n", toml_string(&data.telegram_bot_token)));
        let users = parse_comma_list(&data.telegram_allowed_users);
        out.push_str(&format!("allowed_users = {}\n", toml_string_array(&users)));
        out.push('\n');
    }

    // Slack
    if data.enable_slack {
        out.push_str("[slack]\n");
        out.push_str(&format!("app_token = {}\n", toml_string(&data.slack_app_token)));
        out.push_str(&format!("bot_token = {}\n", toml_string(&data.slack_bot_token)));
        let users = parse_comma_list(&data.slack_allowed_users);
        out.push_str(&format!("allowed_users = {}\n", toml_string_array(&users)));
        out.push('\n');
    }

    // Sandbox
    out.push_str("[sandbox]\n");
    match data.sandbox_provider {
        SandboxProvider::Docker => out.push_str("provider = \"docker\"\n"),
        SandboxProvider::Ecs => out.push_str("provider = \"ecs\"\n"),
    }
    out.push('\n');

    // Docker (always included so the config parses)
    out.push_str("[docker]\n");
    out.push_str(&format!("image = {}\n", toml_string(&data.docker_image)));
    out.push_str(&format!("port_range_start = {}\n", data.docker_port_start));
    out.push_str(&format!("port_range_end = {}\n", data.docker_port_end));
    out.push_str(&format!("network = {}\n", toml_string(&data.docker_network)));
    out.push('\n');

    // ECS (if selected)
    if data.sandbox_provider == SandboxProvider::Ecs {
        out.push_str("[ecs]\n");
        out.push_str(&format!("cluster = {}\n", toml_string(&data.ecs_cluster)));
        out.push_str(&format!(
            "task_definition = {}\n",
            toml_string(&data.ecs_task_definition)
        ));
        let subnets = parse_comma_list(&data.ecs_subnets);
        out.push_str(&format!("subnets = {}\n", toml_string_array(&subnets)));
        let sgs = parse_comma_list(&data.ecs_security_groups);
        if !sgs.is_empty() {
            out.push_str(&format!("security_groups = {}\n", toml_string_array(&sgs)));
        }
        if !data.ecs_region.is_empty() {
            out.push_str(&format!("region = {}\n", toml_string(&data.ecs_region)));
        }
        out.push('\n');
    }

    // Database
    out.push_str("[database]\n");
    out.push_str("path = \"anycode.db\"\n");
    out.push('\n');

    // Agents
    out.push_str("[agents]\n");
    out.push_str(&format!("default_agent = {}\n", toml_string(&data.default_agent)));
    out.push('\n');

    if data.enable_claude_code {
        out.push_str("[agents.credentials.claude-code]\n");
        out.push_str(&format!(
            "env = {{ ANTHROPIC_API_KEY = {} }}\n",
            toml_string(&data.anthropic_api_key)
        ));
        out.push('\n');
    }

    if data.enable_codex {
        out.push_str("[agents.credentials.codex]\n");
        out.push_str(&format!(
            "env = {{ OPENAI_API_KEY = {} }}\n",
            toml_string(&data.openai_api_key)
        ));
        out.push('\n');
    }

    if data.enable_goose {
        out.push_str("[agents.credentials.goose]\n");
        out.push_str(&format!(
            "env = {{ OPENAI_API_KEY = {} }}\n",
            toml_string(&data.openai_api_key)
        ));
        out.push('\n');
    }

    // GitHub
    if !data.github_token.is_empty() {
        out.push_str("[github]\n");
        out.push_str(&format!("token = {}\n", toml_string(&data.github_token)));
        out.push('\n');
    }

    // Session
    out.push_str("[session]\n");
    out.push_str("max_concurrent = 5\n");
    out.push_str("timeout_minutes = 30\n");
    out.push_str("debounce_ms = 500\n");

    out
}

/// Mask a secret for display: show first 4 and last 3 chars.
pub fn mask_secret(s: &str) -> String {
    if s.len() <= 7 {
        return "*".repeat(s.len());
    }
    format!("{}...{}", &s[..4], &s[s.len() - 3..])
}

fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn toml_string_array(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }
    let inner: Vec<String> = items.iter().map(|s| toml_string(s)).collect();
    format!("[{}]", inner.join(", "))
}

fn parse_comma_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_secret_long() {
        assert_eq!(mask_secret("sk-ant-abcdefgh123"), "sk-a...123");
    }

    #[test]
    fn test_mask_secret_short() {
        assert_eq!(mask_secret("abc"), "***");
    }

    #[test]
    fn test_generate_config_minimal() {
        let mut data = WizardData::default();
        data.enable_telegram = true;
        data.telegram_bot_token = "123:ABC".into();
        data.enable_claude_code = true;
        data.anthropic_api_key = "sk-test".into();
        data.default_agent = "claude-code".into();

        let config = generate_config(&data);
        assert!(config.contains("[telegram]"));
        assert!(config.contains("bot_token = \"123:ABC\""));
        assert!(config.contains("[agents.credentials.claude-code]"));
        assert!(config.contains("ANTHROPIC_API_KEY = \"sk-test\""));
        // Slack should not appear
        assert!(!config.contains("[slack]"));
    }

    #[test]
    fn test_parse_comma_list() {
        assert_eq!(
            parse_comma_list("a, b, c"),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert!(parse_comma_list("").is_empty());
    }
}

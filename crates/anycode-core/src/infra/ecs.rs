use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region};
use aws_sdk_cloudwatchlogs::Client as CloudWatchLogsClient;
use aws_sdk_ec2::Client as Ec2Client;
use aws_sdk_ecs::types::{
    AssignPublicIp, AwsVpcConfiguration, ContainerOverride, KeyValuePair, LaunchType,
    NetworkConfiguration, TaskOverride,
};
use aws_sdk_ecs::{types::Task, Client as EcsClient};
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::error::{AnycodeError, Result};

use super::traits::{SandboxConfig, SandboxHandle, SandboxProvider};

#[derive(Debug, Clone)]
pub struct EcsProviderConfig {
    pub cluster: String,
    pub task_definition: String,
    pub subnets: Vec<String>,
    pub security_groups: Vec<String>,
    pub assign_public_ip: bool,
    pub container_port: u16,
    pub startup_timeout_secs: u64,
    pub poll_interval_ms: u64,
    pub region: Option<String>,
    pub platform_version: Option<String>,
    pub container_name: Option<String>,
    pub log_group: Option<String>,
    pub log_stream_prefix: Option<String>,
}

impl EcsProviderConfig {
    fn validate(&self) -> Result<()> {
        if self.cluster.trim().is_empty() {
            return Err(AnycodeError::Config(
                "ecs.cluster cannot be empty".to_string(),
            ));
        }
        if self.task_definition.trim().is_empty() {
            return Err(AnycodeError::Config(
                "ecs.task_definition cannot be empty".to_string(),
            ));
        }
        if self.subnets.is_empty() {
            return Err(AnycodeError::Config(
                "ecs.subnets must include at least one subnet".to_string(),
            ));
        }
        if self.container_port == 0 {
            return Err(AnycodeError::Config(
                "ecs.container_port must be greater than 0".to_string(),
            ));
        }
        Ok(())
    }
}

pub struct EcsFargateProvider {
    ecs: EcsClient,
    ec2: Ec2Client,
    logs: CloudWatchLogsClient,
    config: EcsProviderConfig,
    resolved_container_name: RwLock<Option<String>>,
}

impl EcsFargateProvider {
    pub async fn new(config: EcsProviderConfig) -> Result<Self> {
        config.validate()?;

        let mut loader = aws_config::defaults(BehaviorVersion::latest());
        if let Some(region) = config.region.clone() {
            loader = loader.region(Region::new(region));
        }
        let shared = loader.load().await;

        Ok(Self {
            ecs: EcsClient::new(&shared),
            ec2: Ec2Client::new(&shared),
            logs: CloudWatchLogsClient::new(&shared),
            config,
            resolved_container_name: RwLock::new(None),
        })
    }

    async fn resolve_container_name(&self) -> Result<String> {
        if let Some(name) = self
            .resolved_container_name
            .read()
            .await
            .as_ref()
            .cloned()
        {
            return Ok(name);
        }

        if let Some(name) = self
            .config
            .container_name
            .as_ref()
            .filter(|v| !v.trim().is_empty())
            .cloned()
        {
            let mut w = self.resolved_container_name.write().await;
            *w = Some(name.clone());
            return Ok(name);
        }

        let resp = self
            .ecs
            .describe_task_definition()
            .task_definition(&self.config.task_definition)
            .send()
            .await
            .map_err(|e| {
                AnycodeError::Sandbox(format!("ecs describe_task_definition failed: {e}"))
            })?;

        let container_name = resp
            .task_definition
            .and_then(|td| {
                td.container_definitions
                    .and_then(|defs| defs.into_iter().next())
            })
            .and_then(|def| def.name)
            .ok_or_else(|| {
                AnycodeError::Sandbox(
                    "could not infer ecs container name from task definition".to_string(),
                )
            })?;

        let mut w = self.resolved_container_name.write().await;
        *w = Some(container_name.clone());

        Ok(container_name)
    }

    async fn wait_for_task_url(&self, task_arn: &str) -> Result<String> {
        let started = tokio::time::Instant::now();
        let timeout = Duration::from_secs(self.config.startup_timeout_secs);
        let poll_interval = Duration::from_millis(self.config.poll_interval_ms.max(250));

        loop {
            if started.elapsed() > timeout {
                return Err(AnycodeError::Timeout(format!(
                    "ecs task {task_arn} did not become running in {}s",
                    self.config.startup_timeout_secs
                )));
            }

            let describe = self
                .ecs
                .describe_tasks()
                .cluster(&self.config.cluster)
                .tasks(task_arn)
                .send()
                .await
                .map_err(|e| AnycodeError::Sandbox(format!("ecs describe_tasks failed: {e}")))?;

            let task = describe
                .tasks()
                .first()
                .ok_or_else(|| AnycodeError::NotFound(format!("ecs task not found: {task_arn}")))?;

            let status = task.last_status().unwrap_or("UNKNOWN");
            debug!("ecs task {task_arn} status={status}");

            if status == "STOPPED" {
                let reason = task.stopped_reason().unwrap_or("unknown reason");
                return Err(AnycodeError::Sandbox(format!(
                    "ecs task stopped before ready: {reason}"
                )));
            }

            if status == "RUNNING" {
                if let Some(host) = self.resolve_task_host(task).await? {
                    return Ok(format!("http://{}:{}", host, self.config.container_port));
                }
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    async fn resolve_task_host(&self, task: &Task) -> Result<Option<String>> {
        let eni_id = match extract_network_interface_id(task) {
            Some(eni) => eni,
            None => return Ok(None),
        };

        let resp = self
            .ec2
            .describe_network_interfaces()
            .network_interface_ids(eni_id)
            .send()
            .await
            .map_err(|e| {
                AnycodeError::Sandbox(format!("ec2 describe_network_interfaces failed: {e}"))
            })?;

        let iface = match resp.network_interfaces().first() {
            Some(v) => v,
            None => return Ok(None),
        };

        let private_ip = iface.private_ip_address().map(ToString::to_string);
        let public_ip = iface
            .association()
            .and_then(|assoc| assoc.public_ip())
            .map(ToString::to_string);

        let host = if self.config.assign_public_ip {
            public_ip.or(private_ip)
        } else {
            private_ip.or(public_ip)
        };

        Ok(host)
    }
}

#[async_trait]
impl SandboxProvider for EcsFargateProvider {
    async fn create_sandbox(&self, config: SandboxConfig) -> Result<SandboxHandle> {
        let container_name = self.resolve_container_name().await?;
        let task_overrides = build_task_overrides(
            &container_name,
            &config.agent,
            config.repo_url.as_deref(),
            &config.env,
        );

        let run = self
            .ecs
            .run_task()
            .cluster(&self.config.cluster)
            .task_definition(&self.config.task_definition)
            .launch_type(LaunchType::Fargate)
            .network_configuration(build_network_configuration(
                &self.config.subnets,
                &self.config.security_groups,
                self.config.assign_public_ip,
            )?)
            .overrides(task_overrides)
            .started_by("anycode")
            .client_token(uuid::Uuid::new_v4().to_string());

        let run = if let Some(platform_version) = self.config.platform_version.as_deref() {
            run.platform_version(platform_version)
        } else {
            run
        };

        let resp = run
            .send()
            .await
            .map_err(|e| AnycodeError::Sandbox(format!("ecs run_task failed: {e}")))?;

        if !resp.failures().is_empty() {
            let reasons = resp
                .failures()
                .iter()
                .map(|f| {
                    format!(
                        "{}: {}",
                        f.arn().unwrap_or("unknown"),
                        f.reason().unwrap_or("unknown")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            return Err(AnycodeError::Sandbox(format!("ecs run_task failures: {reasons}")));
        }

        let task_arn = resp
            .tasks()
            .first()
            .and_then(|t| t.task_arn())
            .map(ToString::to_string)
            .ok_or_else(|| AnycodeError::Sandbox("ecs run_task returned no task ARN".to_string()))?;

        info!("Started ECS task {task_arn}");
        let api_url = self.wait_for_task_url(&task_arn).await?;

        Ok(SandboxHandle {
            sandbox_id: task_arn,
            api_url,
            port: self.config.container_port,
        })
    }

    async fn destroy_sandbox(&self, sandbox_id: &str) -> Result<()> {
        self.ecs
            .stop_task()
            .cluster(&self.config.cluster)
            .task(sandbox_id)
            .reason("anycode cleanup")
            .send()
            .await
            .map_err(|e| AnycodeError::Sandbox(format!("ecs stop_task failed: {e}")))?;
        Ok(())
    }

    async fn is_alive(&self, sandbox_id: &str) -> Result<bool> {
        let describe = self
            .ecs
            .describe_tasks()
            .cluster(&self.config.cluster)
            .tasks(sandbox_id)
            .send()
            .await
            .map_err(|e| AnycodeError::Sandbox(format!("ecs describe_tasks failed: {e}")))?;

        let Some(task) = describe.tasks().first() else {
            return Ok(false);
        };
        let status = task.last_status().unwrap_or("UNKNOWN");
        Ok(status != "STOPPED")
    }

    async fn get_logs(&self, sandbox_id: &str, tail: usize) -> Result<String> {
        let Some(log_group) = self.config.log_group.as_deref() else {
            return Ok("CloudWatch logs are not configured (ecs.log_group missing).".to_string());
        };

        let task_id = extract_task_id(sandbox_id);
        let container_name = self.resolve_container_name().await?;
        let stream_name = build_log_stream_name(
            self.config.log_stream_prefix.as_deref(),
            &container_name,
            &task_id,
        );

        let limit = i32::try_from(tail.min(10_000)).unwrap_or(10_000);
        let resp = self
            .logs
            .get_log_events()
            .log_group_name(log_group)
            .log_stream_name(stream_name)
            .limit(limit)
            .start_from_head(false)
            .send()
            .await;

        let resp = match resp {
            Ok(v) => v,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("ResourceNotFoundException") {
                    return Ok(String::new());
                }
                return Err(AnycodeError::Sandbox(format!(
                    "cloudwatch get_log_events failed: {msg}"
                )));
            }
        };

        let output = resp
            .events()
            .iter()
            .filter_map(|e| e.message())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(output)
    }
}

fn build_network_configuration(
    subnets: &[String],
    security_groups: &[String],
    assign_public_ip: bool,
) -> Result<NetworkConfiguration> {
    let vpc = AwsVpcConfiguration::builder()
        .set_subnets(Some(subnets.to_vec()))
        .set_security_groups(Some(security_groups.to_vec()))
        .assign_public_ip(if assign_public_ip {
            AssignPublicIp::Enabled
        } else {
            AssignPublicIp::Disabled
        })
        .build()
        .map_err(|e| AnycodeError::Internal(format!("invalid ecs vpc configuration: {e}")))?;

    Ok(NetworkConfiguration::builder()
        .awsvpc_configuration(vpc)
        .build())
}

fn build_task_overrides(
    container_name: &str,
    agent: &str,
    repo_url: Option<&str>,
    env: &HashMap<String, String>,
) -> TaskOverride {
    let mut env_vars: Vec<KeyValuePair> = env
        .iter()
        .map(|(k, v)| KeyValuePair::builder().name(k).value(v).build())
        .collect();

    env_vars.push(
        KeyValuePair::builder()
            .name("ANYCODE_AGENT")
            .value(agent)
            .build(),
    );
    if let Some(repo) = repo_url {
        env_vars.push(
            KeyValuePair::builder()
                .name("ANYCODE_REPO")
                .value(repo)
                .build(),
        );
    }

    let container_override = ContainerOverride::builder()
        .name(container_name)
        .set_environment(Some(env_vars))
        .build();

    TaskOverride::builder()
        .container_overrides(container_override)
        .build()
}

fn extract_network_interface_id(task: &Task) -> Option<String> {
    for attachment in task.attachments() {
        for detail in attachment.details() {
            if detail.name() == Some("networkInterfaceId") {
                if let Some(value) = detail.value() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn extract_task_id(task_arn: &str) -> String {
    task_arn.rsplit('/').next().unwrap_or(task_arn).to_string()
}

fn build_log_stream_name(
    log_stream_prefix: Option<&str>,
    container_name: &str,
    task_id: &str,
) -> String {
    match log_stream_prefix {
        Some(prefix) if !prefix.trim().is_empty() => {
            format!("{prefix}/{container_name}/{task_id}")
        }
        _ => format!("{container_name}/{task_id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_task_id() {
        let task_id = extract_task_id(
            "arn:aws:ecs:us-west-2:123456789012:task/anycode-cluster/abcdef0123456789",
        );
        assert_eq!(task_id, "abcdef0123456789");
    }

    #[test]
    fn test_build_log_stream_name_with_prefix() {
        let stream = build_log_stream_name(Some("anycode"), "sandbox", "task123");
        assert_eq!(stream, "anycode/sandbox/task123");
    }

    #[test]
    fn test_build_log_stream_name_without_prefix() {
        let stream = build_log_stream_name(None, "sandbox", "task123");
        assert_eq!(stream, "sandbox/task123");
    }

    #[test]
    fn test_build_task_overrides_includes_anycode_env() {
        let mut env = HashMap::new();
        env.insert("OPENAI_API_KEY".to_string(), "test".to_string());
        let overrides = build_task_overrides(
            "sandbox",
            "codex",
            Some("https://github.com/org/repo"),
            &env,
        );

        let container = overrides.container_overrides().first().unwrap();
        let environment = container.environment();
        assert!(
            environment
                .iter()
                .any(|kv: &KeyValuePair| {
                    kv.name() == Some("ANYCODE_AGENT") && kv.value() == Some("codex")
                })
        );
        assert!(
            environment
                .iter()
                .any(|kv: &KeyValuePair| kv.name() == Some("ANYCODE_REPO"))
        );
    }
}

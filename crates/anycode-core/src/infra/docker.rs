use std::collections::HashMap;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
#[allow(deprecated)]
use bollard::container::{
    Config, CreateContainerOptions, InspectContainerOptions, LogsOptions,
    RemoveContainerOptions, StartContainerOptions,
};
use bollard::models::{HostConfig, PortBinding};
use bollard::Docker;
use futures::StreamExt;
use tracing::{error, info};

use crate::error::{AnycodeError, Result};

use super::traits::{SandboxConfig, SandboxHandle, SandboxProvider};

/// Port allocator that hands out unique host ports from a range.
struct PortAllocator {
    next: AtomicU16,
    start: u16,
    end: u16,
}

impl PortAllocator {
    fn new(start: u16, end: u16) -> Self {
        Self {
            next: AtomicU16::new(start),
            start,
            end,
        }
    }

    fn allocate(&self) -> Result<u16> {
        let port = self.next.fetch_add(1, Ordering::SeqCst);
        if port >= self.end {
            self.next.store(self.start, Ordering::SeqCst);
            return Err(AnycodeError::Sandbox(
                "port range exhausted, wrapping around".into(),
            ));
        }
        Ok(port)
    }
}

pub struct DockerProvider {
    docker: Docker,
    image: String,
    port_allocator: Arc<PortAllocator>,
}

impl DockerProvider {
    pub fn new(image: &str, port_start: u16, port_end: u16) -> Result<Self> {
        let docker =
            Docker::connect_with_local_defaults().map_err(AnycodeError::Docker)?;
        Ok(Self {
            docker,
            image: image.to_string(),
            port_allocator: Arc::new(PortAllocator::new(port_start, port_end)),
        })
    }

    /// List all containers with the anycode label.
    pub async fn list_anycode_containers(&self) -> Result<Vec<String>> {
        let mut filters = HashMap::new();
        filters.insert("label", vec!["anycode=true"]);

        #[allow(deprecated)]
        let containers = self
            .docker
            .list_containers(Some(bollard::container::ListContainersOptions {
                all: true,
                filters,
                ..Default::default()
            }))
            .await?;
        Ok(containers.into_iter().filter_map(|c| c.id).collect())
    }
}

#[async_trait]
impl SandboxProvider for DockerProvider {
    async fn create_sandbox(&self, config: SandboxConfig) -> Result<SandboxHandle> {
        let port = self.port_allocator.allocate()?;

        let mut env_vars: Vec<String> = config
            .env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();

        env_vars.push(format!("ANYCODE_AGENT={}", config.agent));

        if let Some(ref repo) = config.repo_url {
            env_vars.push(format!("ANYCODE_REPO={repo}"));
        }

        let container_port = "2468/tcp";
        let mut port_bindings = HashMap::new();
        port_bindings.insert(
            container_port.to_string(),
            Some(vec![PortBinding {
                host_ip: Some("127.0.0.1".to_string()),
                host_port: Some(port.to_string()),
            }]),
        );

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            ..Default::default()
        };

        let mut exposed_ports: HashMap<String, HashMap<(), ()>> = HashMap::new();
        exposed_ports.insert(container_port.to_string(), HashMap::new());

        let mut labels = HashMap::new();
        labels.insert("anycode".to_string(), "true".to_string());
        labels.insert("anycode.agent".to_string(), config.agent.clone());

        let container_name = format!(
            "anycode-{}",
            uuid::Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap()
        );

        let container_config = Config {
            image: Some(self.image.clone()),
            env: Some(env_vars),
            host_config: Some(host_config),
            exposed_ports: Some(exposed_ports),
            labels: Some(labels),
            ..Default::default()
        };

        let create_opts = CreateContainerOptions {
            name: &container_name,
            platform: None,
        };

        info!(
            "Creating container {container_name} with image {} on port {port}",
            self.image
        );

        let response = self
            .docker
            .create_container(Some(create_opts), container_config)
            .await?;
        let container_id = response.id;

        self.docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await?;

        info!("Started container {container_id} on port {port}");

        Ok(SandboxHandle {
            sandbox_id: container_id,
            api_url: format!("http://127.0.0.1:{port}"),
            port,
        })
    }

    async fn destroy_sandbox(&self, sandbox_id: &str) -> Result<()> {
        info!("Destroying container {sandbox_id}");

        #[allow(deprecated)]
        let opts = RemoveContainerOptions {
            force: true,
            v: false,
            link: false,
        };

        self.docker.remove_container(sandbox_id, Some(opts)).await?;
        Ok(())
    }

    async fn is_alive(&self, sandbox_id: &str) -> Result<bool> {
        match self
            .docker
            .inspect_container(sandbox_id, None::<InspectContainerOptions>)
            .await
        {
            Ok(info) => {
                let running = info.state.and_then(|s| s.running).unwrap_or(false);
                Ok(running)
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    async fn get_logs(&self, sandbox_id: &str, tail: usize) -> Result<String> {
        #[allow(deprecated)]
        let opts = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            tail: tail.to_string(),
            ..Default::default()
        };

        let mut stream = self.docker.logs(sandbox_id, Some(opts));
        let mut output = String::new();

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(log) => output.push_str(&log.to_string()),
                Err(e) => {
                    error!("Error reading logs: {e}");
                    break;
                }
            }
        }

        Ok(output)
    }
}

use async_trait::async_trait;

use crate::error::Result;

use super::docker::DockerProvider;
use super::ecs::EcsFargateProvider;
use super::traits::{SandboxConfig, SandboxHandle, SandboxProvider};

pub enum AnySandboxProvider {
    Docker(DockerProvider),
    Ecs(EcsFargateProvider),
}

impl From<DockerProvider> for AnySandboxProvider {
    fn from(value: DockerProvider) -> Self {
        Self::Docker(value)
    }
}

impl From<EcsFargateProvider> for AnySandboxProvider {
    fn from(value: EcsFargateProvider) -> Self {
        Self::Ecs(value)
    }
}

#[async_trait]
impl SandboxProvider for AnySandboxProvider {
    async fn create_sandbox(&self, config: SandboxConfig) -> Result<SandboxHandle> {
        match self {
            Self::Docker(provider) => provider.create_sandbox(config).await,
            Self::Ecs(provider) => provider.create_sandbox(config).await,
        }
    }

    async fn destroy_sandbox(&self, sandbox_id: &str) -> Result<()> {
        match self {
            Self::Docker(provider) => provider.destroy_sandbox(sandbox_id).await,
            Self::Ecs(provider) => provider.destroy_sandbox(sandbox_id).await,
        }
    }

    async fn is_alive(&self, sandbox_id: &str) -> Result<bool> {
        match self {
            Self::Docker(provider) => provider.is_alive(sandbox_id).await,
            Self::Ecs(provider) => provider.is_alive(sandbox_id).await,
        }
    }

    async fn get_logs(&self, sandbox_id: &str, tail: usize) -> Result<String> {
        match self {
            Self::Docker(provider) => provider.get_logs(sandbox_id, tail).await,
            Self::Ecs(provider) => provider.get_logs(sandbox_id, tail).await,
        }
    }
}

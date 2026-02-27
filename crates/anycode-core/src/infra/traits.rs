use async_trait::async_trait;
use std::collections::HashMap;

use crate::error::Result;

/// Configuration for creating a sandbox container.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub image: String,
    pub agent: String,
    pub env: HashMap<String, String>,
    pub repo_url: Option<String>,
}

/// Handle returned after sandbox creation.
#[derive(Debug, Clone)]
pub struct SandboxHandle {
    pub sandbox_id: String,
    pub api_url: String,
    pub port: u16,
}

#[async_trait]
pub trait SandboxProvider: Send + Sync + 'static {
    /// Create a new sandbox container. Returns a handle with connection info.
    async fn create_sandbox(&self, config: SandboxConfig) -> Result<SandboxHandle>;

    /// Destroy a sandbox container.
    async fn destroy_sandbox(&self, sandbox_id: &str) -> Result<()>;

    /// Check if a sandbox container is alive.
    async fn is_alive(&self, sandbox_id: &str) -> Result<bool>;

    /// Get logs from a sandbox container.
    async fn get_logs(&self, sandbox_id: &str, tail: usize) -> Result<String>;
}

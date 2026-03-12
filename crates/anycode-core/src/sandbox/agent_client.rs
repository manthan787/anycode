use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::error::Result;

use super::types::SandboxEvent;

/// Abstraction over agent communication protocols.
///
/// The existing HTTP/SSE path is `OpenCodeClient`; the new `AcpxClient`
/// runs acpx inside Docker containers via `docker exec` and parses NDJSON.
#[async_trait]
pub trait AgentClient: Send + Sync {
    /// Wait until the agent backend is ready to accept requests.
    async fn wait_for_ready(&self, timeout: Duration) -> Result<()>;

    /// Create a new session.
    async fn create_session(&self, id: &str, agent: Option<&str>) -> Result<()>;

    /// Send a message/prompt to a session.
    async fn send_message(&self, session_id: &str, text: &str) -> Result<()>;

    /// Reply to a question from the agent.
    async fn reply_question(&self, question_id: &str, answer: &str) -> Result<()>;

    /// Reply to a permission request from the agent.
    async fn reply_permission(&self, permission_id: &str, approved: bool) -> Result<()>;

    /// Destroy a session.
    async fn destroy_session(&self, session_id: &str) -> Result<()>;

    /// Subscribe to events from the agent. Returns a channel receiver
    /// that yields `SandboxEvent`s as they arrive.
    async fn subscribe_events(&self) -> Result<mpsc::UnboundedReceiver<Result<SandboxEvent>>>;
}

use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::error::{AnycodeError, Result};

use super::agent_client::AgentClient;
use super::stream::{spawn_event_consumer, StreamConfig};
use super::types::*;

/// HTTP client for communicating with agents via the OpenCode REST/SSE protocol
/// (Sandbox Agent SDK).
pub struct OpenCodeClient {
    base_url: String,
    client: Client,
}

/// Backwards-compatible alias.
pub type SandboxClient = OpenCodeClient;

impl OpenCodeClient {
    pub fn new(base_url: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to create HTTP client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        }
    }

    /// Get the SSE event stream URL for a session.
    pub fn event_stream_url(&self) -> String {
        format!("{}/opencode/event", self.base_url)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

#[async_trait]
impl AgentClient for OpenCodeClient {
    async fn wait_for_ready(&self, timeout: Duration) -> Result<()> {
        let start = tokio::time::Instant::now();
        let poll_interval = Duration::from_millis(500);

        loop {
            if start.elapsed() > timeout {
                return Err(AnycodeError::Timeout(
                    "sandbox agent did not become ready".into(),
                ));
            }

            match self
                .client
                .get(format!("{}/v1/health", self.base_url))
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    info!("Sandbox agent is ready at {}", self.base_url);
                    return Ok(());
                }
                Ok(resp) => {
                    debug!("Health check returned {}", resp.status());
                }
                Err(e) => {
                    debug!("Health check failed: {e}");
                }
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    async fn create_session(&self, id: &str, agent: Option<&str>) -> Result<()> {
        let req = CreateSessionRequest {
            id: id.to_string(),
            agent: agent.map(|s| s.to_string()),
        };

        let resp = self
            .client
            .post(format!("{}/opencode/session", self.base_url))
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AnycodeError::Sandbox(format!(
                "create_session failed: {body}"
            )));
        }

        info!("Created session {id}");
        Ok(())
    }

    async fn send_message(&self, session_id: &str, text: &str) -> Result<()> {
        let req = SendMessageRequest {
            message: text.to_string(),
        };

        let resp = self
            .client
            .post(format!(
                "{}/opencode/session/{session_id}/message",
                self.base_url
            ))
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AnycodeError::Sandbox(format!(
                "send_message failed: {body}"
            )));
        }

        debug!("Sent message to session {session_id}");
        Ok(())
    }

    async fn reply_question(&self, question_id: &str, answer: &str) -> Result<()> {
        let req = QuestionReplyRequest {
            answer: answer.to_string(),
        };

        let resp = self
            .client
            .post(format!(
                "{}/opencode/question/{question_id}/reply",
                self.base_url
            ))
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AnycodeError::Sandbox(format!(
                "reply_question failed: {body}"
            )));
        }

        debug!("Replied to question {question_id}");
        Ok(())
    }

    async fn reply_permission(&self, permission_id: &str, approved: bool) -> Result<()> {
        let req = PermissionReplyRequest { approved };

        let resp = self
            .client
            .post(format!(
                "{}/opencode/permission/{permission_id}/reply",
                self.base_url
            ))
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AnycodeError::Sandbox(format!(
                "reply_permission failed: {body}"
            )));
        }

        debug!("Replied to permission {permission_id} with approved={approved}");
        Ok(())
    }

    async fn destroy_session(&self, session_id: &str) -> Result<()> {
        let resp = self
            .client
            .delete(format!(
                "{}/opencode/session/{session_id}",
                self.base_url
            ))
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            warn!("destroy_session returned error: {body}");
        }

        debug!("Destroyed session {session_id}");
        Ok(())
    }

    async fn subscribe_events(&self) -> Result<mpsc::UnboundedReceiver<Result<SandboxEvent>>> {
        let url = self.event_stream_url();
        Ok(spawn_event_consumer(url, StreamConfig::default()))
    }
}

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::config::AppConfig;
use crate::db::{Repository, SessionStatus};
use crate::infra::SandboxProvider;

/// Background watchdog that monitors session timeouts and orphaned containers.
pub struct SessionWatchdog<S: SandboxProvider> {
    config: AppConfig,
    repo: Repository,
    sandbox_provider: Arc<S>,
    shutdown_rx: watch::Receiver<bool>,
}

impl<S: SandboxProvider> SessionWatchdog<S> {
    pub fn new(
        config: AppConfig,
        repo: Repository,
        sandbox_provider: Arc<S>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            config,
            repo,
            sandbox_provider,
            shutdown_rx,
        }
    }

    /// Run the watchdog loop. Checks every 60 seconds.
    pub async fn run(&mut self) {
        let check_interval = Duration::from_secs(60);

        loop {
            tokio::select! {
                _ = tokio::time::sleep(check_interval) => {
                    if let Err(e) = self.check_sessions().await {
                        error!("Watchdog check failed: {e}");
                    }
                }
                _ = self.shutdown_rx.changed() => {
                    if *self.shutdown_rx.borrow() {
                        info!("Watchdog shutting down");
                        return;
                    }
                }
            }
        }
    }

    async fn check_sessions(&self) -> crate::error::Result<()> {
        let sessions = self.repo.get_all_running_sessions().await?;
        let timeout = Duration::from_secs(self.config.session.timeout_minutes * 60);

        for session in sessions {
            // Check timeout
            if let Ok(created) = chrono::DateTime::parse_from_rfc3339(&session.created_at) {
                let elapsed = chrono::Utc::now().signed_duration_since(created);
                if elapsed.to_std().unwrap_or(Duration::ZERO) > timeout {
                    warn!("Session {} timed out, destroying", session.id);
                    if let Some(ref sandbox_id) = session.sandbox_id {
                        let _ = self.sandbox_provider.destroy_sandbox(sandbox_id).await;
                    }
                    self.repo
                        .update_session_status(&session.id, SessionStatus::Failed)
                        .await?;
                    continue;
                }
            }

            // Check if container is still alive
            if let Some(ref sandbox_id) = session.sandbox_id {
                match self.sandbox_provider.is_alive(sandbox_id).await {
                    Ok(false) => {
                        warn!(
                            "Session {} container {} is dead, marking failed",
                            session.id, sandbox_id
                        );
                        self.repo
                            .update_session_status(&session.id, SessionStatus::Failed)
                            .await?;
                    }
                    Err(e) => {
                        warn!("Failed to check container {}: {e}", sandbox_id);
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }
}

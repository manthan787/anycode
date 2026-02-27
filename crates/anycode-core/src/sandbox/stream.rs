use std::time::Duration;

use futures::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::error::{AnycodeError, Result};

use super::types::SandboxEvent;

/// Configuration for the SSE stream consumer.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub max_retries: usize,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
        }
    }
}

/// Spawns an SSE consumer that reconnects on failure with exponential backoff.
/// Returns a channel receiver that yields parsed SandboxEvents.
pub fn spawn_event_consumer(
    event_url: String,
    config: StreamConfig,
) -> mpsc::UnboundedReceiver<Result<SandboxEvent>> {
    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        let mut retries = 0;
        let mut backoff = config.initial_backoff;

        loop {
            info!("Connecting to SSE stream: {event_url}");

            let mut es = EventSource::get(&event_url);

            loop {
                match StreamExt::next(&mut es).await {
                    Some(Ok(Event::Open)) => {
                        info!("SSE stream connected");
                        retries = 0;
                        backoff = config.initial_backoff;
                    }
                    Some(Ok(Event::Message(msg))) => {
                        debug!("SSE event: type={}, data={}", msg.event, msg.data);

                        match serde_json::from_str::<SandboxEvent>(&msg.data) {
                            Ok(event) => {
                                if tx.send(Ok(event)).is_err() {
                                    info!("Event consumer channel closed, stopping");
                                    return;
                                }
                            }
                            Err(e) => {
                                warn!("Failed to parse SSE event: {e}, data: {}", msg.data);
                            }
                        }
                    }
                    Some(Err(e)) => {
                        warn!("SSE stream error: {e}");
                        es.close();
                        break;
                    }
                    None => {
                        info!("SSE stream ended");
                        break;
                    }
                }
            }

            // Reconnection logic
            retries += 1;
            if retries > config.max_retries {
                error!(
                    "SSE max retries ({}) exceeded, giving up",
                    config.max_retries
                );
                let _ = tx.send(Err(AnycodeError::Sandbox(
                    "SSE stream disconnected after max retries".into(),
                )));
                return;
            }

            warn!(
                "SSE disconnected, retry {retries}/{} in {backoff:?}",
                config.max_retries
            );
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(config.max_backoff);
        }
    });

    rx
}

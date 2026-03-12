use std::time::Duration;

use async_trait::async_trait;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::Docker;
use futures::StreamExt;
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info, warn};

use crate::error::{AnycodeError, Result};

use super::agent_client::AgentClient;
use super::types::SandboxEvent;

/// NDJSON envelope emitted by `acpx --format json`.
#[derive(Debug, Clone, Deserialize)]
pub struct AcpxEnvelope {
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub args: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
}

/// Agent client that communicates via acpx inside Docker containers.
///
/// Instead of HTTP/SSE, this runs `docker exec acpx ...` and parses NDJSON
/// output line-by-line, converting each line into `SandboxEvent`s.
pub struct AcpxClient {
    docker: Docker,
    container_id: String,
    agent: String,
    /// Sender half of the internal event channel.
    tx: mpsc::UnboundedSender<Result<SandboxEvent>>,
    /// Receiver half — taken once by subscribe_events().
    rx: Mutex<Option<mpsc::UnboundedReceiver<Result<SandboxEvent>>>>,
    /// Handle to abort the running exec task.
    exec_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl AcpxClient {
    pub fn new(docker: Docker, container_id: String, agent: String) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            docker,
            container_id,
            agent,
            tx,
            rx: Mutex::new(Some(rx)),
            exec_handle: Mutex::new(None),
        }
    }
}

#[async_trait]
impl AgentClient for AcpxClient {
    async fn wait_for_ready(&self, timeout: Duration) -> Result<()> {
        let start = tokio::time::Instant::now();
        let poll_interval = Duration::from_millis(500);

        loop {
            if start.elapsed() > timeout {
                return Err(AnycodeError::Timeout(
                    "acpx not found in container".into(),
                ));
            }

            let exec = self
                .docker
                .create_exec(
                    &self.container_id,
                    CreateExecOptions {
                        cmd: Some(vec!["which", "acpx"]),
                        attach_stdout: Some(true),
                        attach_stderr: Some(true),
                        ..Default::default()
                    },
                )
                .await;

            match exec {
                Ok(created) => {
                    if let Ok(StartExecResults::Attached { mut output, .. }) =
                        self.docker.start_exec(&created.id, None).await
                    {
                        let mut found = false;
                        while let Some(Ok(chunk)) = output.next().await {
                            let text = chunk.to_string();
                            if text.contains("acpx") {
                                found = true;
                            }
                        }
                        if found {
                            info!("acpx is available in container {}", self.container_id);
                            return Ok(());
                        }
                    }
                }
                Err(e) => {
                    debug!("acpx check failed: {e}");
                }
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    async fn create_session(&self, _id: &str, _agent: Option<&str>) -> Result<()> {
        // acpx sessions are implicit — no-op
        debug!("acpx: create_session is a no-op (sessions are implicit)");
        Ok(())
    }

    async fn send_message(&self, session_id: &str, text: &str) -> Result<()> {
        let session_id = session_id.to_string();
        let text = text.to_string();
        let docker = self.docker.clone();
        let container_id = self.container_id.clone();
        let agent = self.agent.clone();
        let tx = self.tx.clone();

        let handle = tokio::spawn(async move {
            // Emit synthetic SessionStarted
            let _ = tx.send(Ok(SandboxEvent::SessionStarted {
                session_id: session_id.clone(),
            }));

            let cmd = vec![
                "acpx".to_string(),
                "--format".to_string(),
                "json".to_string(),
                "--approve-all".to_string(),
                agent,
                text,
            ];

            let exec = match docker
                .create_exec(
                    &container_id,
                    CreateExecOptions {
                        cmd: Some(cmd.iter().map(|s| s.as_str()).collect()),
                        attach_stdout: Some(true),
                        attach_stderr: Some(true),
                        ..Default::default()
                    },
                )
                .await
            {
                Ok(e) => e,
                Err(e) => {
                    let _ = tx.send(Err(AnycodeError::Sandbox(format!(
                        "acpx exec create failed: {e}"
                    ))));
                    return;
                }
            };

            match docker.start_exec(&exec.id, None).await {
                Ok(StartExecResults::Attached { mut output, .. }) => {
                    let mut line_buf = String::new();
                    let mut item_counter: u64 = 0;

                    while let Some(chunk_result) = output.next().await {
                        match chunk_result {
                            Ok(chunk) => {
                                let chunk_str = chunk.to_string();
                                line_buf.push_str(&chunk_str);

                                // Process complete lines
                                while let Some(newline_pos) = line_buf.find('\n') {
                                    let line: String =
                                        line_buf.drain(..=newline_pos).collect();
                                    let line = line.trim();
                                    if line.is_empty() {
                                        continue;
                                    }

                                    let events = parse_acpx_line(
                                        line,
                                        &session_id,
                                        &mut item_counter,
                                    );
                                    for event in events {
                                        if tx.send(Ok(event)).is_err() {
                                            return;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("acpx output stream error: {e}");
                                break;
                            }
                        }
                    }

                    // Process any remaining partial line
                    let remaining = line_buf.trim();
                    if !remaining.is_empty() {
                        let events =
                            parse_acpx_line(remaining, &session_id, &mut item_counter);
                        for event in events {
                            if tx.send(Ok(event)).is_err() {
                                return;
                            }
                        }
                    }
                }
                Ok(StartExecResults::Detached) => {
                    warn!("acpx exec started in detached mode unexpectedly");
                }
                Err(e) => {
                    let _ = tx.send(Err(AnycodeError::Sandbox(format!(
                        "acpx exec start failed: {e}"
                    ))));
                    return;
                }
            }

            // Emit synthetic SessionEnded
            let _ = tx.send(Ok(SandboxEvent::SessionEnded {
                session_id: session_id.clone(),
            }));
        });

        let mut exec_handle = self.exec_handle.lock().await;
        *exec_handle = Some(handle);

        Ok(())
    }

    async fn reply_question(&self, question_id: &str, _answer: &str) -> Result<()> {
        warn!(
            "acpx: reply_question({question_id}) is a no-op (running with --approve-all)"
        );
        Ok(())
    }

    async fn reply_permission(&self, permission_id: &str, _approved: bool) -> Result<()> {
        warn!(
            "acpx: reply_permission({permission_id}) is a no-op (running with --approve-all)"
        );
        Ok(())
    }

    async fn destroy_session(&self, _session_id: &str) -> Result<()> {
        let mut handle = self.exec_handle.lock().await;
        if let Some(h) = handle.take() {
            h.abort();
            debug!("acpx: aborted running exec task");
        }
        Ok(())
    }

    async fn subscribe_events(&self) -> Result<mpsc::UnboundedReceiver<Result<SandboxEvent>>> {
        let mut rx = self.rx.lock().await;
        rx.take().ok_or_else(|| {
            AnycodeError::Sandbox("acpx event receiver already taken".into())
        })
    }
}

/// Parse a single NDJSON line from acpx into zero or more `SandboxEvent`s.
fn parse_acpx_line(
    line: &str,
    session_id: &str,
    item_counter: &mut u64,
) -> Vec<SandboxEvent> {
    let envelope: AcpxEnvelope = match serde_json::from_str(line) {
        Ok(e) => e,
        Err(e) => {
            debug!("acpx: skipping unparseable line: {e} — {line}");
            return vec![];
        }
    };

    let mut events = Vec::new();

    match envelope.r#type.as_str() {
        "text" => {
            if let Some(content) = envelope.content {
                if !content.is_empty() {
                    *item_counter += 1;
                    let item_id = format!("acpx-item-{item_counter}");
                    events.push(SandboxEvent::ItemDelta {
                        session_id: session_id.to_string(),
                        item_id,
                        delta: content,
                    });
                }
            }
        }
        "thinking" => {
            *item_counter += 1;
            let item_id = format!("acpx-item-{item_counter}");
            let content = envelope.content.unwrap_or_default();

            events.push(SandboxEvent::ItemStarted {
                session_id: session_id.to_string(),
                item_id: item_id.clone(),
                item_type: Some("thinking".to_string()),
            });
            if !content.is_empty() {
                events.push(SandboxEvent::ItemDelta {
                    session_id: session_id.to_string(),
                    item_id: item_id.clone(),
                    delta: content.clone(),
                });
            }
            events.push(SandboxEvent::ItemCompleted {
                session_id: session_id.to_string(),
                item_id,
                content: if content.is_empty() {
                    None
                } else {
                    Some(content)
                },
            });
        }
        "tool_call" => {
            *item_counter += 1;
            let item_id = format!("acpx-item-{item_counter}");
            let tool_name = envelope.tool.unwrap_or_else(|| "unknown".to_string());
            let description = format!(
                "Tool: {tool_name}{}",
                envelope
                    .args
                    .as_deref()
                    .map(|a| format!("\nArgs: {a}"))
                    .unwrap_or_default()
            );

            events.push(SandboxEvent::ItemStarted {
                session_id: session_id.to_string(),
                item_id: item_id.clone(),
                item_type: Some("tool_call".to_string()),
            });
            events.push(SandboxEvent::ItemCompleted {
                session_id: session_id.to_string(),
                item_id,
                content: Some(description),
            });
        }
        "done" => {
            // No event — the outer task emits SessionEnded after the stream closes.
            debug!("acpx: received done event");
        }
        other => {
            debug!("acpx: skipping unknown event type: {other}");
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_event() {
        let line = r#"{"type":"text","content":"Hello world"}"#;
        let mut counter = 0;
        let events = parse_acpx_line(line, "sess-1", &mut counter);
        assert_eq!(events.len(), 1);
        assert_eq!(counter, 1);
        match &events[0] {
            SandboxEvent::ItemDelta { delta, session_id, .. } => {
                assert_eq!(delta, "Hello world");
                assert_eq!(session_id, "sess-1");
            }
            other => panic!("expected ItemDelta, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_text_event_empty_content() {
        let line = r#"{"type":"text","content":""}"#;
        let mut counter = 0;
        let events = parse_acpx_line(line, "sess-1", &mut counter);
        assert!(events.is_empty());
        assert_eq!(counter, 0);
    }

    #[test]
    fn test_parse_thinking_event() {
        let line = r#"{"type":"thinking","content":"Let me think..."}"#;
        let mut counter = 0;
        let events = parse_acpx_line(line, "sess-1", &mut counter);
        assert_eq!(events.len(), 3);
        assert_eq!(counter, 1);

        assert!(matches!(&events[0], SandboxEvent::ItemStarted { item_type, .. }
            if item_type.as_deref() == Some("thinking")));
        assert!(matches!(&events[1], SandboxEvent::ItemDelta { delta, .. }
            if delta == "Let me think..."));
        assert!(matches!(&events[2], SandboxEvent::ItemCompleted { content, .. }
            if content.as_deref() == Some("Let me think...")));
    }

    #[test]
    fn test_parse_thinking_event_empty_content() {
        let line = r#"{"type":"thinking"}"#;
        let mut counter = 0;
        let events = parse_acpx_line(line, "sess-1", &mut counter);
        // ItemStarted + ItemCompleted (no delta for empty content)
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_parse_tool_call_event() {
        let line = r#"{"type":"tool_call","tool":"read_file","args":"{\"path\":\"src/main.rs\"}"}"#;
        let mut counter = 0;
        let events = parse_acpx_line(line, "sess-1", &mut counter);
        assert_eq!(events.len(), 2);

        assert!(matches!(&events[0], SandboxEvent::ItemStarted { item_type, .. }
            if item_type.as_deref() == Some("tool_call")));
        match &events[1] {
            SandboxEvent::ItemCompleted { content, .. } => {
                let c = content.as_ref().unwrap();
                assert!(c.contains("read_file"));
                assert!(c.contains("src/main.rs"));
            }
            other => panic!("expected ItemCompleted, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_done_event() {
        let line = r#"{"type":"done"}"#;
        let mut counter = 0;
        let events = parse_acpx_line(line, "sess-1", &mut counter);
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_unknown_event() {
        let line = r#"{"type":"some_future_type","data":"foo"}"#;
        let mut counter = 0;
        let events = parse_acpx_line(line, "sess-1", &mut counter);
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_malformed_json() {
        let line = "not json at all";
        let mut counter = 0;
        let events = parse_acpx_line(line, "sess-1", &mut counter);
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_unknown_fields_ignored() {
        let line = r#"{"type":"text","content":"hi","unknown_field":42,"nested":{"a":1}}"#;
        let mut counter = 0;
        let events = parse_acpx_line(line, "sess-1", &mut counter);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], SandboxEvent::ItemDelta { delta, .. } if delta == "hi"));
    }

    #[test]
    fn test_item_counter_increments() {
        let mut counter = 0;
        let _ = parse_acpx_line(r#"{"type":"text","content":"a"}"#, "s", &mut counter);
        assert_eq!(counter, 1);
        let _ = parse_acpx_line(r#"{"type":"text","content":"b"}"#, "s", &mut counter);
        assert_eq!(counter, 2);
        let _ = parse_acpx_line(r#"{"type":"thinking","content":"c"}"#, "s", &mut counter);
        assert_eq!(counter, 3);
    }

    #[test]
    fn test_tool_call_without_args() {
        let line = r#"{"type":"tool_call","tool":"list_files"}"#;
        let mut counter = 0;
        let events = parse_acpx_line(line, "sess-1", &mut counter);
        assert_eq!(events.len(), 2);
        match &events[1] {
            SandboxEvent::ItemCompleted { content, .. } => {
                let c = content.as_ref().unwrap();
                assert!(c.contains("list_files"));
                assert!(!c.contains("Args:"));
            }
            other => panic!("expected ItemCompleted, got {other:?}"),
        }
    }
}

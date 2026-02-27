use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::time::Instant;
use tracing::{debug, error, info, warn};

use crate::config::AppConfig;
use crate::db::{
    InteractionKind, PendingInteraction, Repository, Session, SessionStatus,
};
use crate::error::Result;
use crate::infra::SandboxProvider;
use crate::messaging::{InboundEvent, MessagingProvider, OutboundMessage};
use crate::sandbox::{SandboxClient, SandboxEvent};
use crate::sandbox::stream::{spawn_event_consumer, StreamConfig};

/// Buffers delta text and flushes to Telegram on a debounce timer.
pub struct DeltaBuffer {
    buffer: String,
    current_message_id: Option<i64>,
    last_flush: Instant,
    debounce: Duration,
    max_message_len: usize,
}

impl DeltaBuffer {
    pub fn new(debounce_ms: u64) -> Self {
        Self {
            buffer: String::new(),
            current_message_id: None,
            last_flush: Instant::now(),
            debounce: Duration::from_millis(debounce_ms),
            max_message_len: 3800,
        }
    }

    pub fn append(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    pub fn should_flush(&self) -> bool {
        !self.buffer.is_empty() && self.last_flush.elapsed() >= self.debounce
    }

    pub fn needs_new_message(&self) -> bool {
        self.buffer.len() > self.max_message_len
    }

    pub fn take_flush(&mut self) -> Option<(String, Option<i64>)> {
        if self.buffer.is_empty() {
            return None;
        }

        // If buffer is too long, we need a new message
        if self.needs_new_message() {
            let text = self.buffer.clone();
            self.buffer.clear();
            self.current_message_id = None;
            self.last_flush = Instant::now();
            return Some((text, None)); // None = send new message
        }

        let text = self.buffer.clone();
        let edit_id = self.current_message_id;
        self.last_flush = Instant::now();
        Some((text, edit_id))
    }

    pub fn set_message_id(&mut self, id: i64) {
        self.current_message_id = Some(id);
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.current_message_id = None;
    }
}

/// Active session state tracked in memory.
struct ActiveSession {
    session: Session,
    sandbox_client: SandboxClient,
    delta_buffer: tokio::sync::Mutex<DeltaBuffer>,
}

/// The control bridge connects Telegram events to Sandbox Agent sessions.
pub struct Bridge<M: MessagingProvider, S: SandboxProvider> {
    config: AppConfig,
    messaging: Arc<M>,
    sandbox_provider: Arc<S>,
    repo: Repository,
    /// Map of session_id -> active session state.
    active_sessions: Arc<DashMap<String, Arc<ActiveSession>>>,
    /// Map of chat_id -> most recent active session_id.
    chat_sessions: Arc<DashMap<i64, String>>,
}

impl<M: MessagingProvider, S: SandboxProvider> Bridge<M, S> {
    pub fn new(
        config: AppConfig,
        messaging: Arc<M>,
        sandbox_provider: Arc<S>,
        repo: Repository,
    ) -> Self {
        Self {
            config,
            messaging,
            sandbox_provider,
            repo,
            active_sessions: Arc::new(DashMap::new()),
            chat_sessions: Arc::new(DashMap::new()),
        }
    }

    /// Start processing inbound events from the messaging platform.
    pub async fn run(self: Arc<Self>) -> Result<()> {
        // Recover any orphaned sessions on startup
        self.recover_sessions().await?;

        let mut rx = self.messaging.subscribe().await?;

        info!("Bridge started, listening for events");

        while let Some(event) = rx.recv().await {
            let bridge = Arc::clone(&self);
            tokio::spawn(async move {
                if let Err(e) = bridge.handle_event(event).await {
                    error!("Error handling event: {e}");
                }
            });
        }

        warn!("Inbound event stream ended");
        Ok(())
    }

    async fn handle_event(&self, event: InboundEvent) -> Result<()> {
        match event {
            InboundEvent::Command {
                chat_id,
                user_id,
                command,
                args,
            } => {
                if !self.is_user_allowed(user_id) {
                    self.send_text(chat_id, "You are not authorized to use this bot.")
                        .await?;
                    return Ok(());
                }
                self.handle_command(chat_id, user_id, &command, &args).await
            }
            InboundEvent::Message {
                chat_id,
                user_id,
                text,
            } => {
                if !self.is_user_allowed(user_id) {
                    self.send_text(chat_id, "You are not authorized to use this bot.")
                        .await?;
                    return Ok(());
                }
                self.handle_message(chat_id, user_id, &text).await
            }
            InboundEvent::CallbackQuery {
                query_id,
                chat_id,
                user_id,
                message_id,
                data,
            } => {
                if !self.is_user_allowed(user_id) {
                    self.messaging
                        .answer_callback(&query_id, "You are not authorized to use this bot.")
                        .await?;
                    return Ok(());
                }
                self.handle_callback(query_id, chat_id, user_id, message_id, &data)
                    .await
            }
        }
    }

    async fn handle_command(
        &self,
        chat_id: i64,
        _user_id: i64,
        command: &str,
        args: &str,
    ) -> Result<()> {
        match command {
            "task" | "start" => self.cmd_task(chat_id, args).await,
            "status" => self.cmd_status(chat_id).await,
            "cancel" => self.cmd_cancel(chat_id, args).await,
            "agents" => self.cmd_agents(chat_id).await,
            "help" => self.cmd_help(chat_id).await,
            _ => {
                self.send_text(chat_id, "Unknown command. Try /help.")
                    .await?;
                Ok(())
            }
        }
    }

    fn is_user_allowed(&self, user_id: i64) -> bool {
        self.config.telegram.allowed_users.is_empty()
            || self.config.telegram.allowed_users.contains(&user_id)
    }

    async fn handle_message(&self, chat_id: i64, _user_id: i64, text: &str) -> Result<()> {
        // Route plain text to the most recent active session in this chat
        if let Some(session_id) = self.chat_sessions.get(&chat_id).map(|v| v.clone()) {
            if let Some(active) = self.active_sessions.get(&session_id) {
                let unresolved = self.repo.get_unresolved_for_session(&session_id).await?;
                if let Some((pi_id, question_id)) =
                    get_pending_free_form_question(&unresolved)
                {
                    active
                        .sandbox_client
                        .reply_question(&question_id, text)
                        .await?;
                    self.repo.resolve_pending_interaction(&pi_id).await?;
                    return Ok(());
                }

                active
                    .sandbox_client
                    .send_message(&session_id, text)
                    .await?;
                return Ok(());
            }
        }

        self.send_text(chat_id, "No active session. Start one with /task <prompt>")
            .await?;
        Ok(())
    }

    async fn handle_callback(
        &self,
        query_id: String,
        _chat_id: i64,
        _user_id: i64,
        _message_id: i64,
        data: &str,
    ) -> Result<()> {
        // Callback data format: "q:<interaction_id>:<answer>" or "p:<interaction_id>:<approved>"
        let parts: Vec<&str> = data.splitn(3, ':').collect();
        if parts.len() < 3 {
            self.messaging.answer_callback(&query_id, "Invalid callback").await?;
            return Ok(());
        }

        let (kind, interaction_id, value) = (parts[0], parts[1], parts[2]);

        let pi = self.repo.get_pending_interaction(interaction_id).await?;
        let pi = match pi {
            Some(pi) if !pi.resolved => pi,
            _ => {
                self.messaging
                    .answer_callback(&query_id, "Already resolved")
                    .await?;
                return Ok(());
            }
        };

        if let Some(active) = self.active_sessions.get(&pi.session_id) {
            match kind {
                "q" => {
                    if let Some(ref qid) = pi.question_id {
                        active.sandbox_client.reply_question(qid, value).await?;
                    }
                }
                "p" => {
                    if let Some(ref pid) = pi.permission_id {
                        let approved = value == "yes";
                        active
                            .sandbox_client
                            .reply_permission(pid, approved)
                            .await?;
                    }
                }
                _ => {}
            }
        }

        self.repo.resolve_pending_interaction(interaction_id).await?;
        self.messaging.answer_callback(&query_id, "OK").await?;

        Ok(())
    }

    // --- Commands ---

    async fn cmd_task(&self, chat_id: i64, args: &str) -> Result<()> {
        if args.trim().is_empty() {
            self.send_text(chat_id, "Usage: /task [agent] <prompt>")
                .await?;
            return Ok(());
        }

        // Check concurrent session limit
        let active = self.repo.get_active_sessions_for_chat(chat_id).await?;
        if active.len() >= self.config.session.max_concurrent {
            self.send_text(
                chat_id,
                &format!(
                    "Too many active sessions ({}). Cancel one first with /cancel.",
                    active.len()
                ),
            )
            .await?;
            return Ok(());
        }

        // Parse agent and prompt
        let known = self.config.known_agents();
        let mut words = args.splitn(2, ' ');
        let first = words.next().unwrap_or("");
        let rest = words.next().unwrap_or("");

        let (agent, prompt) = if known.iter().any(|a| a == first) {
            (first.to_string(), rest.to_string())
        } else {
            (self.config.agents.default_agent.clone(), args.to_string())
        };

        if prompt.trim().is_empty() {
            self.send_text(chat_id, "Please provide a prompt after the agent name.")
                .await?;
            return Ok(());
        }

        // Detect repo URL
        let repo_url = extract_repo_url(&prompt);

        let session_id = uuid::Uuid::new_v4().to_string();

        let session = Session {
            id: session_id.clone(),
            chat_id,
            agent: agent.clone(),
            prompt: prompt.clone(),
            repo_url: repo_url.clone(),
            sandbox_id: None,
            sandbox_port: None,
            session_api_id: None,
            status: SessionStatus::Pending,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        self.repo.create_session(&session).await?;

        let short_id = &session_id[..8];
        self.send_text(
            chat_id,
            &format!("Starting {agent} session `{short_id}`...\nPrompt: {prompt}"),
        )
        .await?;

        // Spawn the session lifecycle
        let bridge = Arc::new(BridgeRef {
            config: self.config.clone(),
            messaging: Arc::clone(&self.messaging),
            sandbox_provider: Arc::clone(&self.sandbox_provider),
            repo: self.repo.clone(),
            active_sessions: Arc::clone(&self.active_sessions),
            chat_sessions: Arc::clone(&self.chat_sessions),
        });

        tokio::spawn(async move {
            if let Err(e) = run_session(bridge, session).await {
                error!("Session {session_id} failed: {e}");
            }
        });

        Ok(())
    }

    async fn cmd_status(&self, chat_id: i64) -> Result<()> {
        let sessions = self.repo.get_active_sessions_for_chat(chat_id).await?;
        if sessions.is_empty() {
            self.send_text(chat_id, "No active sessions.").await?;
            return Ok(());
        }

        let mut text = String::from("Active sessions:\n");
        for s in &sessions {
            let short_id = &s.id[..8];
            text.push_str(&format!(
                "- `{short_id}` | {} | {} | {}\n",
                s.agent,
                s.status.as_str(),
                s.created_at
            ));
        }

        self.send_text(chat_id, &text).await?;
        Ok(())
    }

    async fn cmd_cancel(&self, chat_id: i64, args: &str) -> Result<()> {
        let sessions = self.repo.get_active_sessions_for_chat(chat_id).await?;
        let session_id = if args.trim().is_empty() {
            sessions.first().map(|s| s.id.clone())
        } else {
            // Find session matching prefix
            let prefix = args.trim();
            sessions
                .iter()
                .find(|s| s.id.starts_with(prefix))
                .map(|s| s.id.clone())
        };

        match session_id {
            Some(sid) => {
                self.cancel_session(&sid).await?;
                self.send_text(chat_id, &format!("Session `{}` cancelled.", &sid[..8]))
                    .await?;
            }
            None => {
                self.send_text(chat_id, "No active session to cancel.")
                    .await?;
            }
        }

        Ok(())
    }

    async fn cmd_agents(&self, chat_id: i64) -> Result<()> {
        let agents = self.config.known_agents();
        let default = &self.config.agents.default_agent;
        let mut text = String::from("Available agents:\n");
        for a in &agents {
            if a == default {
                text.push_str(&format!("- {a} (default)\n"));
            } else {
                text.push_str(&format!("- {a}\n"));
            }
        }
        self.send_text(chat_id, &text).await?;
        Ok(())
    }

    async fn cmd_help(&self, chat_id: i64) -> Result<()> {
        let text = "\
Anycode Bot - Run coding agents from Telegram

Commands:
/task [agent] <prompt> - Start a coding task
/status - List active sessions
/cancel [id] - Cancel a session
/agents - List available agents
/help - Show this message

Plain text messages are forwarded to your most recent active session.
Button presses answer agent questions/permissions.";

        self.send_text(chat_id, text).await?;
        Ok(())
    }

    // --- Helpers ---

    async fn send_text(&self, chat_id: i64, text: &str) -> Result<i64> {
        self.messaging
            .send_message(OutboundMessage {
                chat_id,
                text: text.to_string(),
                edit_message_id: None,
                buttons: vec![],
            })
            .await
    }

    async fn cancel_session(&self, session_id: &str) -> Result<()> {
        if let Some((_, active)) = self.active_sessions.remove(session_id) {
            let _ = active.sandbox_client.destroy_session(session_id).await;
            if let Some(ref sid) = active.session.sandbox_id {
                let _ = self.sandbox_provider.destroy_sandbox(sid).await;
            }
        }

        self.repo
            .update_session_status(session_id, SessionStatus::Cancelled)
            .await?;

        // Clean up chat_sessions mapping
        let session = self.repo.get_session(session_id).await?;
        if let Some(s) = session {
            self.chat_sessions.remove_if(&s.chat_id, |_, v| v == session_id);
        }

        Ok(())
    }

    async fn recover_sessions(&self) -> Result<()> {
        let running = self.repo.get_all_running_sessions().await?;
        for session in running {
            if let Some(ref sandbox_id) = session.sandbox_id {
                let alive = self.sandbox_provider.is_alive(sandbox_id).await.unwrap_or(false);
                if alive {
                    info!("Reattaching to session {}", session.id);
                    // Could reattach SSE consumer here — for now just mark alive
                } else {
                    warn!("Session {} container is dead, marking failed", session.id);
                    self.repo
                        .update_session_status(&session.id, SessionStatus::Failed)
                        .await?;
                }
            } else {
                self.repo
                    .update_session_status(&session.id, SessionStatus::Failed)
                    .await?;
            }
        }
        Ok(())
    }

    /// Destroy all active sessions (called on shutdown).
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down bridge, destroying all active containers");
        let sessions: Vec<_> = self
            .active_sessions
            .iter()
            .map(|e| e.key().clone())
            .collect();

        for session_id in sessions {
            if let Err(e) = self.cancel_session(&session_id).await {
                error!("Failed to cancel session {session_id}: {e}");
            }
        }
        Ok(())
    }
}

/// Shared references for spawned session tasks.
struct BridgeRef<M: MessagingProvider, S: SandboxProvider> {
    config: AppConfig,
    messaging: Arc<M>,
    sandbox_provider: Arc<S>,
    repo: Repository,
    active_sessions: Arc<DashMap<String, Arc<ActiveSession>>>,
    chat_sessions: Arc<DashMap<i64, String>>,
}

/// Run a complete session lifecycle.
async fn run_session<M: MessagingProvider, S: SandboxProvider>(
    bridge: Arc<BridgeRef<M, S>>,
    mut session: Session,
) -> Result<()> {
    let session_id = session.id.clone();
    let chat_id = session.chat_id;

    // Update status to starting
    bridge
        .repo
        .update_session_status(&session_id, SessionStatus::Starting)
        .await?;

    // 1. Create sandbox container
    let env = bridge
        .config
        .agents
        .credentials
        .get(&session.agent)
        .map(|c| c.env.clone())
        .unwrap_or_default();

    let sandbox_config = crate::infra::SandboxConfig {
        image: bridge.config.docker.image.clone(),
        agent: session.agent.clone(),
        env,
        repo_url: session.repo_url.clone(),
    };

    let handle = match bridge.sandbox_provider.create_sandbox(sandbox_config).await {
        Ok(h) => h,
        Err(e) => {
            error!("Failed to create sandbox: {e}");
            bridge
                .repo
                .update_session_status(&session_id, SessionStatus::Failed)
                .await?;
            send_text(&bridge.messaging, chat_id, &format!("Failed to start: {e}")).await?;
            return Err(e);
        }
    };

    bridge
        .repo
        .update_session_sandbox(&session_id, &handle.sandbox_id, handle.port)
        .await?;
    session.sandbox_id = Some(handle.sandbox_id.clone());
    session.sandbox_port = Some(handle.port as i64);

    // 2. Wait for sandbox agent to be ready
    let client = SandboxClient::new(&handle.api_url);
    if let Err(e) = client.wait_for_ready(Duration::from_secs(60)).await {
        error!("Sandbox agent not ready: {e}");
        let _ = bridge.sandbox_provider.destroy_sandbox(&handle.sandbox_id).await;
        bridge
            .repo
            .update_session_status(&session_id, SessionStatus::Failed)
            .await?;
        send_text(&bridge.messaging, chat_id, &format!("Sandbox startup failed: {e}")).await?;
        return Err(e);
    }

    // 3. Create session in sandbox agent
    if let Err(e) = client
        .create_session(&session_id, Some(&session.agent))
        .await
    {
        error!("Failed to create sandbox session: {e}");
        let _ = bridge.sandbox_provider.destroy_sandbox(&handle.sandbox_id).await;
        bridge
            .repo
            .update_session_status(&session_id, SessionStatus::Failed)
            .await?;
        send_text(&bridge.messaging, chat_id, &format!("Session creation failed: {e}")).await?;
        return Err(e);
    }

    bridge
        .repo
        .update_session_api_id(&session_id, &session_id)
        .await?;

    // 4. Update status to running
    bridge
        .repo
        .update_session_status(&session_id, SessionStatus::Running)
        .await?;

    // 5. Track active session
    let active = Arc::new(ActiveSession {
        session: session.clone(),
        sandbox_client: SandboxClient::new(&handle.api_url),
        delta_buffer: tokio::sync::Mutex::new(DeltaBuffer::new(
            bridge.config.session.debounce_ms,
        )),
    });

    bridge
        .active_sessions
        .insert(session_id.clone(), Arc::clone(&active));
    bridge.chat_sessions.insert(chat_id, session_id.clone());

    // 6. Send prompt
    if let Err(e) = active
        .sandbox_client
        .send_message(&session_id, &session.prompt)
        .await
    {
        error!("Failed to send prompt: {e}");
        cleanup_session(&bridge, &session_id, &handle.sandbox_id, SessionStatus::Failed).await;
        send_text(&bridge.messaging, chat_id, &format!("Failed to send prompt: {e}")).await?;
        return Err(e);
    }

    // 7. Consume SSE events
    let event_url = active.sandbox_client.event_stream_url();
    let mut event_rx = spawn_event_consumer(event_url, StreamConfig::default());

    // Spawn a debounce flusher
    let flush_active = Arc::clone(&active);
    let flush_messaging = Arc::clone(&bridge.messaging);
    let debounce_ms = bridge.config.session.debounce_ms;
    let flush_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(debounce_ms));
        loop {
            interval.tick().await;
            let mut buf = flush_active.delta_buffer.lock().await;
            if buf.should_flush() {
                if let Some((text, edit_id)) = buf.take_flush() {
                    match send_stream_chunks(&flush_messaging, chat_id, &text, edit_id).await {
                        Ok(Some(mid)) => buf.set_message_id(mid),
                        Ok(None) => {}
                        Err(e) => warn!("Failed to flush delta: {e}"),
                    }
                }
            }
        }
    });

    // Process events
    let mut final_status = SessionStatus::Failed;
    while let Some(result) = event_rx.recv().await {
        match result {
            Ok(event) => {
                // Log event
                let payload = serde_json::to_string(&event).ok();
                bridge
                    .repo
                    .log_event(&session_id, event.event_type(), payload.as_deref())
                    .await?;

                match event {
                    SandboxEvent::SessionStarted { .. } => {
                        debug!("Session {session_id} started");
                    }
                    SandboxEvent::ItemDelta { delta, .. } => {
                        let mut buf = active.delta_buffer.lock().await;
                        buf.append(&delta);
                    }
                    SandboxEvent::ItemCompleted { content, .. } => {
                        // Flush remaining buffer
                        let mut buf = active.delta_buffer.lock().await;
                        if let Some(ref c) = content {
                            buf.append(c);
                        }
                        if let Some((text, edit_id)) = buf.take_flush() {
                            if let Ok(Some(mid)) =
                                send_stream_chunks(&bridge.messaging, chat_id, &text, edit_id)
                                    .await
                            {
                                buf.set_message_id(mid);
                            }
                        }
                        buf.reset();
                    }
                    SandboxEvent::QuestionRequested {
                        question_id,
                        text,
                        options,
                        ..
                    } => {
                        let pi_id = uuid::Uuid::new_v4().to_string();
                        let buttons: Vec<Vec<(String, String)>> = if options.is_empty() {
                            // Free-form question — no buttons, user types answer
                            vec![]
                        } else {
                            vec![options
                                .iter()
                                .map(|o| {
                                    (
                                        o.label.clone(),
                                        format!("q:{}:{}", pi_id, o.value),
                                    )
                                })
                                .collect()]
                        };

                        let msg_text = format!("Question: {text}");
                        let msg_id = bridge
                            .messaging
                            .send_message(OutboundMessage {
                                chat_id,
                                text: msg_text,
                                edit_message_id: None,
                                buttons,
                            })
                            .await?;

                        let pi = PendingInteraction {
                            id: pi_id,
                            session_id: session_id.clone(),
                            kind: InteractionKind::Question,
                            question_id: Some(question_id),
                            permission_id: None,
                            telegram_message_id: Some(msg_id),
                            payload: if options.is_empty() {
                                Some("free_form".to_string())
                            } else {
                                None
                            },
                            resolved: false,
                            created_at: chrono::Utc::now().to_rfc3339(),
                        };
                        bridge.repo.create_pending_interaction(&pi).await?;
                    }
                    SandboxEvent::PermissionRequested {
                        permission_id,
                        description,
                        command,
                        ..
                    } => {
                        let pi_id = uuid::Uuid::new_v4().to_string();
                        let mut msg_text = format!("Permission requested: {description}");
                        if let Some(ref cmd) = command {
                            msg_text.push_str(&format!("\nCommand: `{cmd}`"));
                        }

                        let buttons = vec![vec![
                            ("Approve".to_string(), format!("p:{pi_id}:yes")),
                            ("Deny".to_string(), format!("p:{pi_id}:no")),
                        ]];

                        let msg_id = bridge
                            .messaging
                            .send_message(OutboundMessage {
                                chat_id,
                                text: msg_text,
                                edit_message_id: None,
                                buttons,
                            })
                            .await?;

                        let pi = PendingInteraction {
                            id: pi_id,
                            session_id: session_id.clone(),
                            kind: InteractionKind::Permission,
                            question_id: None,
                            permission_id: Some(permission_id),
                            telegram_message_id: Some(msg_id),
                            payload: command.map(|c| c.to_string()),
                            resolved: false,
                            created_at: chrono::Utc::now().to_rfc3339(),
                        };
                        bridge.repo.create_pending_interaction(&pi).await?;
                    }
                    SandboxEvent::SessionEnded { .. } => {
                        info!("Session {session_id} ended");
                        final_status = SessionStatus::Completed;
                        send_text(&bridge.messaging, chat_id, "Session completed.").await?;
                        break;
                    }
                    SandboxEvent::Error { message, .. } => {
                        error!("Session {session_id} error: {message}");
                        final_status = SessionStatus::Failed;
                        send_text(
                            &bridge.messaging,
                            chat_id,
                            &format!("Agent error: {message}"),
                        )
                        .await?;
                        break;
                    }
                    _ => {}
                }
            }
            Err(e) => {
                error!("SSE stream error for session {session_id}: {e}");
                final_status = SessionStatus::Failed;

                // Check if container is still alive
                if let Some(ref sid) = session.sandbox_id {
                    if !bridge.sandbox_provider.is_alive(sid).await.unwrap_or(false) {
                        send_text(
                            &bridge.messaging,
                            chat_id,
                            "Container crashed. Session failed.",
                        )
                        .await?;
                    }
                }
                break;
            }
        }
    }

    // Cleanup
    flush_handle.abort();
    cleanup_session(
        &bridge,
        &session_id,
        session.sandbox_id.as_deref().unwrap_or(""),
        final_status,
    )
    .await;

    Ok(())
}

async fn cleanup_session<M: MessagingProvider, S: SandboxProvider>(
    bridge: &BridgeRef<M, S>,
    session_id: &str,
    sandbox_id: &str,
    status: SessionStatus,
) {
    bridge.active_sessions.remove(session_id);
    let chat_keys: Vec<i64> = bridge
        .chat_sessions
        .iter()
        .filter_map(|entry| {
            if entry.value() == session_id {
                Some(*entry.key())
            } else {
                None
            }
        })
        .collect();
    for chat_id in chat_keys {
        bridge
            .chat_sessions
            .remove_if(&chat_id, |_, mapped_session_id| mapped_session_id == session_id);
    }

    if !sandbox_id.is_empty() {
        if let Err(e) = bridge.sandbox_provider.destroy_sandbox(sandbox_id).await {
            error!("Failed to destroy sandbox {sandbox_id}: {e}");
        }
    }

    match bridge
        .repo
        .update_session_status_if_active(session_id, status)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            debug!("Session {session_id} already terminal, skipping status update");
        }
        Err(e) => {
            error!("Failed to update session status: {e}");
        }
    }
}

async fn send_text<M: MessagingProvider>(
    messaging: &Arc<M>,
    chat_id: i64,
    text: &str,
) -> Result<i64> {
    messaging
        .send_message(OutboundMessage {
            chat_id,
            text: text.to_string(),
            edit_message_id: None,
            buttons: vec![],
        })
        .await
}

async fn send_stream_chunks<M: MessagingProvider>(
    messaging: &Arc<M>,
    chat_id: i64,
    text: &str,
    edit_message_id: Option<i64>,
) -> Result<Option<i64>> {
    let chunks = split_message(text, 4096);
    let mut last_message_id = None;

    for (idx, chunk) in chunks.iter().enumerate() {
        let message_id = messaging
            .send_message(OutboundMessage {
                chat_id,
                text: chunk.clone(),
                edit_message_id: if idx == 0 { edit_message_id } else { None },
                buttons: vec![],
            })
            .await?;
        last_message_id = Some(message_id);
    }

    Ok(last_message_id)
}

fn floor_char_boundary(text: &str, max_len: usize) -> usize {
    if text.len() <= max_len {
        return text.len();
    }

    let mut idx = max_len;
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    if idx == 0 {
        text.chars().next().map(|c| c.len_utf8()).unwrap_or(0)
    } else {
        idx
    }
}

fn get_pending_free_form_question(
    interactions: &[PendingInteraction],
) -> Option<(String, String)> {
    interactions
        .iter()
        .find_map(|pi| {
            if pi.kind == InteractionKind::Question
                && pi.payload.as_deref() == Some("free_form")
            {
                pi.question_id
                    .as_ref()
                    .map(|question_id| (pi.id.clone(), question_id.clone()))
            } else {
                None
            }
        })
}

/// Extract a GitHub/GitLab repo URL from text.
fn extract_repo_url(text: &str) -> Option<String> {
    let patterns = [
        r"https?://github\.com/[\w\-\.]+/[\w\-\.]+",
        r"https?://gitlab\.com/[\w\-\.]+/[\w\-\.]+",
        r"[\w\-\.]+/[\w\-\.]+",
    ];

    for pattern in &patterns[..2] {
        if let Ok(re) = regex_lite::Regex::new(pattern) {
            if let Some(m) = re.find(text) {
                return Some(m.as_str().to_string());
            }
        }
    }

    // Simple org/repo pattern
    if let Ok(re) = regex_lite::Regex::new(r"\b([\w\-]+/[\w\-]+)\b") {
        if let Some(m) = re.find(text) {
            let candidate = m.as_str();
            // Filter out common false positives
            if !candidate.contains("//") && candidate.contains('/') {
                let parts: Vec<&str> = candidate.split('/').collect();
                if parts.len() == 2 && parts[0].len() > 1 && parts[1].len() > 1 {
                    return Some(format!("https://github.com/{candidate}"));
                }
            }
        }
    }

    None
}

/// Split a long message into chunks that fit Telegram's limit.
pub fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if max_len == 0 {
        return vec![];
    }
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        let window_end = floor_char_boundary(remaining, max_len);
        let window = &remaining[..window_end];
        let mut split_at = window.rfind('\n').unwrap_or(window_end);
        if split_at == 0 {
            split_at = window_end;
        }

        chunks.push(remaining[..split_at].to_string());
        remaining = &remaining[split_at..].trim_start_matches('\n');
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    use async_trait::async_trait;

    use crate::config::{
        AgentsConfig, AppConfig, DatabaseConfig, DockerConfig, SessionConfig, TelegramConfig,
    };
    use crate::infra::{SandboxConfig, SandboxHandle, SandboxProvider};
    use crate::messaging::traits::MessagingProvider;

    #[derive(Default)]
    struct MockMessaging {
        sent: tokio::sync::Mutex<Vec<OutboundMessage>>,
        callbacks: tokio::sync::Mutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl MessagingProvider for MockMessaging {
        async fn send_message(&self, msg: OutboundMessage) -> Result<i64> {
            self.sent.lock().await.push(msg);
            Ok(1)
        }

        async fn answer_callback(&self, query_id: &str, text: &str) -> Result<()> {
            self.callbacks
                .lock()
                .await
                .push((query_id.to_string(), text.to_string()));
            Ok(())
        }

        async fn subscribe(&self) -> Result<tokio::sync::mpsc::UnboundedReceiver<InboundEvent>> {
            let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
            Ok(rx)
        }

        async fn send_file(&self, _chat_id: i64, _filename: &str, _data: Vec<u8>) -> Result<()> {
            Ok(())
        }
    }

    struct MockSandbox;

    #[async_trait]
    impl SandboxProvider for MockSandbox {
        async fn create_sandbox(&self, _config: SandboxConfig) -> Result<SandboxHandle> {
            Err(crate::error::AnycodeError::Sandbox("not implemented".to_string()))
        }

        async fn destroy_sandbox(&self, _sandbox_id: &str) -> Result<()> {
            Ok(())
        }

        async fn is_alive(&self, _sandbox_id: &str) -> Result<bool> {
            Ok(false)
        }

        async fn get_logs(&self, _sandbox_id: &str, _tail: usize) -> Result<String> {
            Ok(String::new())
        }
    }

    async fn build_bridge(
        allowed_users: Vec<i64>,
    ) -> (
        Arc<Bridge<MockMessaging, MockSandbox>>,
        Arc<MockMessaging>,
    ) {
        let config = AppConfig {
            telegram: TelegramConfig {
                bot_token: "token".to_string(),
                allowed_users,
            },
            docker: DockerConfig {
                image: "anycode-sandbox:latest".to_string(),
                port_range_start: 12000,
                port_range_end: 12100,
                network: "bridge".to_string(),
            },
            database: DatabaseConfig {
                path: ":memory:".to_string(),
            },
            agents: AgentsConfig {
                default_agent: "claude-code".to_string(),
                credentials: HashMap::new(),
            },
            session: SessionConfig::default(),
        };

        let repo = Repository::new_in_memory().await.unwrap();
        let messaging = Arc::new(MockMessaging::default());
        let bridge = Arc::new(Bridge::new(
            config,
            Arc::clone(&messaging),
            Arc::new(MockSandbox),
            repo,
        ));

        (bridge, messaging)
    }

    async fn build_bridge_ref() -> (BridgeRef<MockMessaging, MockSandbox>, Repository) {
        let config = AppConfig {
            telegram: TelegramConfig {
                bot_token: "token".to_string(),
                allowed_users: vec![],
            },
            docker: DockerConfig {
                image: "anycode-sandbox:latest".to_string(),
                port_range_start: 12000,
                port_range_end: 12100,
                network: "bridge".to_string(),
            },
            database: DatabaseConfig {
                path: ":memory:".to_string(),
            },
            agents: AgentsConfig {
                default_agent: "claude-code".to_string(),
                credentials: HashMap::new(),
            },
            session: SessionConfig::default(),
        };

        let repo = Repository::new_in_memory().await.unwrap();
        let bridge = BridgeRef {
            config,
            messaging: Arc::new(MockMessaging::default()),
            sandbox_provider: Arc::new(MockSandbox),
            repo: repo.clone(),
            active_sessions: Arc::new(DashMap::new()),
            chat_sessions: Arc::new(DashMap::new()),
        };

        (bridge, repo)
    }

    #[test]
    fn test_extract_repo_url_github() {
        let url = extract_repo_url("fix bug in https://github.com/org/repo please");
        assert_eq!(url.unwrap(), "https://github.com/org/repo");
    }

    #[test]
    fn test_extract_repo_url_org_repo() {
        let url = extract_repo_url("fix bug in org/repo please");
        assert_eq!(url.unwrap(), "https://github.com/org/repo");
    }

    #[test]
    fn test_split_message() {
        let text = "a".repeat(5000);
        let chunks = split_message(&text, 4096);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].len() <= 4096);
    }

    #[test]
    fn test_split_message_at_newline() {
        let mut text = "a".repeat(3800);
        text.push('\n');
        text.push_str(&"b".repeat(200));
        let chunks = split_message(&text, 4096);
        assert_eq!(chunks.len(), 1); // fits in one message (4001 < 4096)
    }

    #[test]
    fn test_split_message_unicode_safe() {
        let text = "🙂".repeat(3000);
        let chunks = split_message(&text, 4096);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 4096));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn test_delta_buffer() {
        let mut buf = DeltaBuffer::new(500);
        buf.append("hello ");
        buf.append("world");
        assert!(!buf.should_flush()); // debounce not elapsed yet

        // Simulate time passing
        buf.last_flush = Instant::now() - Duration::from_secs(1);
        assert!(buf.should_flush());

        let (text, edit_id) = buf.take_flush().unwrap();
        assert_eq!(text, "hello world");
        assert!(edit_id.is_none()); // no message ID set yet

        buf.set_message_id(42);
        buf.append("more text");
        buf.last_flush = Instant::now() - Duration::from_secs(1);
        let (_, edit_id) = buf.take_flush().unwrap();
        assert_eq!(edit_id, Some(42));
    }

    #[test]
    fn test_get_pending_free_form_question_finds_marked_question() {
        let interactions = vec![
            PendingInteraction {
                id: "pi-1".to_string(),
                session_id: "s1".to_string(),
                kind: InteractionKind::Permission,
                question_id: None,
                permission_id: Some("perm-1".to_string()),
                telegram_message_id: None,
                payload: None,
                resolved: false,
                created_at: "2024-01-01T00:00:00Z".to_string(),
            },
            PendingInteraction {
                id: "pi-2".to_string(),
                session_id: "s1".to_string(),
                kind: InteractionKind::Question,
                question_id: Some("q-1".to_string()),
                permission_id: None,
                telegram_message_id: None,
                payload: Some("free_form".to_string()),
                resolved: false,
                created_at: "2024-01-01T00:00:00Z".to_string(),
            },
        ];

        let selected = get_pending_free_form_question(&interactions);
        assert_eq!(selected, Some(("pi-2".to_string(), "q-1".to_string())));
    }

    #[test]
    fn test_get_pending_free_form_question_ignores_non_free_form() {
        let interactions = vec![PendingInteraction {
            id: "pi-1".to_string(),
            session_id: "s1".to_string(),
            kind: InteractionKind::Question,
            question_id: Some("q-1".to_string()),
            permission_id: None,
            telegram_message_id: None,
            payload: None,
            resolved: false,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }];

        assert_eq!(get_pending_free_form_question(&interactions), None);
    }

    #[tokio::test]
    async fn test_rejects_unauthorized_plain_messages() {
        let (bridge, messaging) = build_bridge(vec![7]).await;

        bridge
            .handle_event(InboundEvent::Message {
                chat_id: 1,
                user_id: 99,
                text: "hello".to_string(),
            })
            .await
            .unwrap();

        let sent = messaging.sent.lock().await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].text, "You are not authorized to use this bot.");
    }

    #[tokio::test]
    async fn test_rejects_unauthorized_callbacks() {
        let (bridge, messaging) = build_bridge(vec![7]).await;

        bridge
            .handle_event(InboundEvent::CallbackQuery {
                query_id: "qid".to_string(),
                chat_id: 1,
                user_id: 99,
                message_id: 1,
                data: "q:abc:yes".to_string(),
            })
            .await
            .unwrap();

        let callbacks = messaging.callbacks.lock().await;
        assert_eq!(callbacks.len(), 1);
        assert_eq!(
            callbacks[0].1,
            "You are not authorized to use this bot."
        );
    }

    #[tokio::test]
    async fn test_send_stream_chunks_first_chunk_can_edit() {
        let messaging = Arc::new(MockMessaging::default());
        let text = "a".repeat(5000);

        let last_id = send_stream_chunks(&messaging, 1, &text, Some(77))
            .await
            .unwrap();

        let sent = messaging.sent.lock().await;
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].edit_message_id, Some(77));
        assert_eq!(sent[1].edit_message_id, None);
        assert!(sent.iter().all(|msg| msg.text.len() <= 4096));
        assert_eq!(last_id, Some(1));
    }

    #[tokio::test]
    async fn test_cleanup_session_does_not_override_terminal_status() {
        let (bridge, repo) = build_bridge_ref().await;

        let session = Session {
            id: "session-1".to_string(),
            chat_id: 1,
            agent: "claude-code".to_string(),
            prompt: "test".to_string(),
            repo_url: None,
            sandbox_id: None,
            sandbox_port: None,
            session_api_id: None,
            status: SessionStatus::Cancelled,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        repo.create_session(&session).await.unwrap();

        cleanup_session(&bridge, &session.id, "", SessionStatus::Completed).await;

        let updated = repo.get_session(&session.id).await.unwrap().unwrap();
        assert_eq!(updated.status, SessionStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_cleanup_session_removes_chat_mapping() {
        let (bridge, _repo) = build_bridge_ref().await;
        bridge
            .chat_sessions
            .insert(42, "session-for-chat".to_string());

        cleanup_session(
            &bridge,
            "session-for-chat",
            "",
            SessionStatus::Completed,
        )
        .await;

        assert!(bridge.chat_sessions.get(&42).is_none());
    }

    #[tokio::test]
    async fn test_cmd_cancel_ignores_stale_chat_mapping() {
        let (bridge, messaging) = build_bridge(vec![]).await;
        let session = Session {
            id: "completed-session".to_string(),
            chat_id: 101,
            agent: "claude-code".to_string(),
            prompt: "test".to_string(),
            repo_url: None,
            sandbox_id: None,
            sandbox_port: None,
            session_api_id: None,
            status: SessionStatus::Completed,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        bridge.repo.create_session(&session).await.unwrap();
        bridge
            .chat_sessions
            .insert(session.chat_id, session.id.clone());

        bridge.cmd_cancel(session.chat_id, "").await.unwrap();

        let updated = bridge.repo.get_session(&session.id).await.unwrap().unwrap();
        assert_eq!(updated.status, SessionStatus::Completed);

        let sent = messaging.sent.lock().await;
        assert_eq!(sent.last().unwrap().text, "No active session to cancel.");
    }
}

# Anycode — Architecture, Decisions & Philosophy

## What is Anycode?

A Rust daemon that bridges messaging platforms (Telegram and Slack) with isolated coding agent sandboxes (Claude Code, Codex, Goose). Users dispatch `/task` commands from chat; Anycode spins up a sandbox (Docker locally or ECS Fargate in cloud) running the agent, streams output back, handles bidirectional Q&A via inline buttons, and tears down the sandbox when done.

---

## Module Map

```
anycode-core/
├── config.rs          TOML config loading & validation
├── error.rs           Unified AnycodeError enum (thiserror)
├── db/
│   ├── models.rs      Session, PendingInteraction, EventLogEntry
│   └── repo.rs        Async SQLite repository (tokio-rusqlite)
├── infra/
│   ├── traits.rs      SandboxProvider trait
│   ├── docker.rs      Docker implementation (bollard)
│   ├── ecs.rs         ECS Fargate implementation (aws-sdk)
│   └── provider.rs    Runtime provider enum (Docker/ECS)
├── messaging/
│   ├── traits.rs      MessagingProvider trait + event types
│   ├── telegram.rs    Teloxide-based Telegram bot
│   └── slack.rs       Slack Socket Mode (WebSocket) bot
├── sandbox/
│   ├── agent_client.rs AgentClient trait (protocol abstraction)
│   ├── types.rs       SandboxEvent enum (10 SSE event types)
│   ├── client.rs      OpenCodeClient: HTTP/SSE via sandbox agent REST API
│   ├── acpx.rs        AcpxClient: NDJSON via docker exec + acpx CLI
│   └── stream.rs      SSE consumer with exponential backoff
├── control/
│   └── bridge.rs      Core orchestration: Messaging Platform <-> Sandbox Agent
└── session/
    └── manager.rs     Background watchdog (timeouts, orphans)

anycode-bin/
└── main.rs            CLI entrypoint (clap, tracing, shutdown)

anycode-setup/
├── main.rs            TUI setup wizard entrypoint
├── app.rs             Application state machine
├── data.rs            Wizard data model
├── config_gen.rs      TOML config generation
├── runner.rs          Subprocess execution (build steps)
├── steps/             Wizard step implementations (welcome → done)
└── widgets/           Reusable TUI input widgets
```

---

## Key Traits (Extensibility Points)

### MessagingProvider
```rust
pub trait MessagingProvider: Send + Sync + 'static {
    async fn send_message(&self, msg: OutboundMessage) -> Result<String>;
    async fn answer_callback(&self, query_id: &str, text: &str) -> Result<()>;
    async fn subscribe(&self) -> Result<mpsc::UnboundedReceiver<InboundEvent>>;
    async fn send_file(&self, chat_id: &str, filename: &str, data: Vec<u8>) -> Result<()>;
}
```
Currently implemented: Telegram (teloxide) and Slack (Socket Mode). Adding Discord/Matrix = just a new impl.

### SandboxProvider
```rust
pub trait SandboxProvider: Send + Sync + 'static {
    async fn create_sandbox(&self, config: SandboxConfig) -> Result<SandboxHandle>;
    async fn destroy_sandbox(&self, sandbox_id: &str) -> Result<()>;
    async fn is_alive(&self, sandbox_id: &str) -> Result<bool>;
    async fn get_logs(&self, sandbox_id: &str, tail: usize) -> Result<String>;
}
```
Currently implemented: Docker (bollard) and ECS Fargate (aws-sdk). Extensible to Kubernetes, EC2, E2B, etc.

### AgentClient
```rust
pub trait AgentClient: Send + Sync {
    async fn wait_for_ready(&self, timeout: Duration) -> Result<()>;
    async fn create_session(&self, id: &str, agent: Option<&str>) -> Result<()>;
    async fn send_message(&self, session_id: &str, text: &str) -> Result<()>;
    async fn reply_question(&self, question_id: &str, answer: &str) -> Result<()>;
    async fn reply_permission(&self, permission_id: &str, approved: bool) -> Result<()>;
    async fn destroy_session(&self, session_id: &str) -> Result<()>;
    async fn subscribe_events(&self) -> Result<mpsc::UnboundedReceiver<Result<SandboxEvent>>>;
}
```
Currently implemented: OpenCodeClient (REST/SSE via sandbox-agent) and AcpxClient (NDJSON via docker exec + acpx). Selected via `sandbox.protocol` config.

---

## Concurrency Model

| Primitive | Used For |
|---|---|
| `Arc<DashMap<K, V>>` | Lock-free concurrent session & chat routing maps |
| `tokio::spawn` | Per-event handling, per-session lifecycle, SSE consumer, delta flusher |
| `tokio::sync::Mutex` | Per-session DeltaBuffer (async-safe) |
| `tokio::sync::watch` | Shutdown broadcast to all tasks |
| `mpsc::unbounded_channel` | Messaging platform event subscription, SSE event delivery |

**Spawned tasks per session:**
1. **Session lifecycle** (run_session) — owns container + event loop
2. **SSE consumer** (spawn_event_consumer) — persistent HTTP connection, reconnects
3. **Delta flusher** — periodic timer, flushes buffered output to messaging platform

All tasks terminate on session end or shutdown signal.

---

## Session Lifecycle (State Machine)

```
Pending → Starting → Running → Completed / Failed / Cancelled
```

1. `/task` received → create Session (Pending)
2. SandboxProvider.create_sandbox() → container/task starts (Starting)
3. AgentClient.wait_for_ready() → poll health/check binary (60s timeout)
4. AgentClient.create_session() + send_message() → agent working (Running)
5. Events stream back (SSE or NDJSON): deltas → Telegram edits, questions → inline buttons
6. session.ended or error → cleanup sandbox, update DB (Completed/Failed)

**Cancellation**: `/cancel` → remove from DashMap, destroy sandbox, mark Cancelled.

**Recovery on startup**: Query DB for non-terminal sessions. Check sandbox alive. Dead → mark Failed.

---

## DeltaBuffer: Debounced Streaming Output

Problem: Messaging platforms rate-limit message edits. Agent produces many small text deltas.

Solution: Buffer deltas, flush on a timer.

- **Append**: `item.delta` events append text to buffer (non-blocking)
- **Flush check**: Every `debounce_ms` (default 500ms), if buffer non-empty and debounce elapsed, send/edit message
- **Message splitting**: When buffer exceeds ~3800 chars, start a new message (Telegram limit is 4096; Slack is more generous)
- **Force flush**: On `item.completed`, immediately flush remaining buffer
- **Markdown fallback**: Try MarkdownV2 first; on parse error, retry as plain text

---

## SSE Stream Reconnection

```
StreamConfig {
    max_retries: 5,
    initial_backoff: 1s,
    max_backoff: 30s,
}
```

- On disconnect: retry with exponential backoff (1s → 2s → 4s → 8s → 16s → 30s cap)
- On successful reconnect: reset retry count and backoff
- After max retries exhausted: send error to channel, stop consumer
- On receiver dropped: consumer task exits (checked via send() return)

---

## Error Handling

Single unified enum via thiserror:

```rust
pub enum AnycodeError {
    Config(String),
    Database(#[from] tokio_rusqlite::Error),
    Docker(#[from] bollard::errors::Error),
    Http(#[from] reqwest::Error),
    Sandbox(String), Messaging(String), Session(String),
    Timeout(String), NotFound(String),
    Json(#[from] serde_json::Error),
    Internal(String),
}
```

**Philosophy:**
- Automatic `From` conversions for library errors — use `?` freely
- Domain-specific variants for semantic matching
- Config validation fails early at load time
- Container failures → mark session Failed + notify user
- Malformed SSE events → log and skip (lenient; don't crash the session)
- No panics in async paths

---

## Database (SQLite)

### Tables
- **sessions**: Maps chat → sandbox → agent session. Tracks full lifecycle.
- **pending_interactions**: Unresolved questions/permissions + Telegram message IDs for callback routing.
- **event_log**: Full SSE event history per session (audit trail).

### Access Pattern
- `tokio-rusqlite`: Executes blocking SQLite calls on a thread pool, returns futures
- Data cloned before moving into closure (thread boundary requirement)
- `rusqlite::params![]` for parameterized queries (SQL injection safe)

### Why SQLite
- No server process, file-based, embedded
- Good for single-host daemon
- In-memory mode for tests

---

## Configuration Philosophy

**Minimal required config**: At least one messaging platform (`telegram.bot_token` or `slack.app_token` + `slack.bot_token`), with default `sandbox.provider = "docker"`.

Everything else has sensible defaults:
- Sandbox provider: `docker`
- Agent protocol: `opencode` (alternative: `acpx`)
- Docker image: `anycode-sandbox:latest`
- Port range: 12000-12100
- Database: `anycode.db`
- Default agent: `claude-code`
- Max concurrent sessions: 5
- Timeout: 30 minutes
- Debounce: 500ms

Agent credentials passed per-agent via `[agents.credentials.<name>]` sections → injected as container env vars at creation time (never baked into image).

When `sandbox.provider = "ecs"`, these are additionally required:
- `ecs.cluster`
- `ecs.task_definition`
- `ecs.subnets` (at least one)

---

## Callback Data Protocol

Inline button callbacks encode interaction type and answer:

```
q:<interaction_id>:<answer_value>   — question reply
p:<interaction_id>:yes|no           — permission approval/denial
```

On button press: look up PendingInteraction by ID, forward answer to sandbox agent, mark resolved.

---

## Known Gotchas & Workarounds

| Issue | Status | Notes |
|---|---|---|
| **Bollard deprecated APIs** | `#[allow(deprecated)]` | bollard 0.19 deprecated old container API in favor of builders. Deprecated API still works fine. Monitor for bollard 0.20+ migration. |
| **ECS startup latency variance** | Works for now | Fargate cold-start time depends on region/image pull. Tuned with `startup_timeout_secs` and `poll_interval_ms`. |
| **Port allocator is naive (Docker)** | Works for now | Simple atomic counter with wraparound. Doesn't track freed ports or in-use collisions. |
| **Session recovery is partial** | Marks dead sessions as Failed | Full reattachment (reconnect SSE to existing container) not implemented. Container alive → still lost. |
| **Telegram Markdown escaping** | Fallback to plain text | MarkdownV2 has strict escape rules. If edit fails, retry without formatting. |
| **DashMap iteration + removal** | Collect keys first | Can't remove during DashMap iteration; collect into Vec, iterate separately. |
| **SSE parse errors** | Log and skip | Malformed events don't crash session. Trade-off: possible silent event loss. |

---

## Testing Strategy

- **Unit tests co-located** with source (`#[cfg(test)]` blocks)
- **In-memory SQLite** for DB tests (fast, isolated)
- **tempfile** for config tests
- **33 tests total**: config (4), DB CRUD (7), bridge behavior (12), infra helpers (6), message/URL/delta utilities (4)
- **No integration tests** — would require Docker daemon + Telegram bot token
- **All async** via `#[tokio::test]`

---

## Dependency Choices

| Crate | Why |
|---|---|
| tokio (full) | Async runtime, timers, signals, sync primitives |
| teloxide | Telegram bot framework with update handler DSL |
| bollard | Low-level Docker API client (more control than docker-rs) |
| aws-config / aws-sdk-ecs / aws-sdk-ec2 / aws-sdk-cloudwatchlogs | ECS Fargate lifecycle, ENI lookup, and CloudWatch logs |
| reqwest + reqwest-eventsource | HTTP client with native SSE support |
| tokio-rusqlite / rusqlite (bundled) | Async SQLite without server overhead |
| dashmap | Lock-free concurrent hashmap (better than RwLock for read-heavy) |
| tracing | Structured async-aware logging |
| thiserror / anyhow | Error derivation (core) / contextual errors (bin) |
| regex-lite | Lightweight regex for URL extraction (no full regex engine) |
| chrono | Timestamp handling (RFC3339 format) |

---

## Design Principles

1. **Trait abstraction over concrete types** — swap messaging and infra backends without touching orchestration
2. **Explicit ownership** — Arc clones are deliberate; no hidden global state
3. **Fail fast on config, recover on runtime** — validation at startup, lenient during operation
4. **Non-blocking everything** — tokio spawns for parallelism, async mutex only where necessary
5. **Operational simplicity** — single binary, file-based DB, optional external infra (Docker or ECS)
6. **Graceful degradation** — SSE reconnects, markdown fallback, orphan cleanup
7. **Audit trail** — event_log captures full session history for debugging

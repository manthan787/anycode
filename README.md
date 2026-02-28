<p align="center">
  <h1 align="center">Anycode</h1>
  <p align="center">
    <strong>Run any coding agent from Telegram.</strong>
  </p>
  <p align="center">
    Dispatch tasks to Claude Code, Codex, or Goose from a chat message.<br>
    Anycode spins up an isolated sandbox (Docker locally or ECS Fargate in cloud), streams output back, handles Q&A via buttons, and cleans up when done.
  </p>
</p>

<p align="center">
  <a href="#quickstart">Quickstart</a> &bull;
  <a href="#how-it-works">How It Works</a> &bull;
  <a href="#commands">Commands</a> &bull;
  <a href="#configuration">Configuration</a> &bull;
  <a href="#architecture">Architecture</a> &bull;
  <a href="#contributing">Contributing</a>
</p>

---

## Why?

No single tool lets you dispatch coding tasks from a messaging app, spin up a sandboxed agent, get streaming output, answer the agent's questions interactively, and tear everything down automatically. Anycode does.

- **Message-driven** &mdash; start tasks from Telegram (Slack/Discord planned)
- **Agent-agnostic** &mdash; Claude Code, Codex, Goose, or any agent behind Rivet's Sandbox Agent SDK
- **Fully sandboxed** &mdash; each task runs in an ephemeral Docker container or ECS Fargate task
- **Bidirectional** &mdash; questions and permission requests appear as inline buttons; your replies go back to the agent
- **Streaming** &mdash; see the agent's output as it types, debounced to avoid rate limits

---

## Quickstart

### Prerequisites

- **Rust** 1.75+ (for building)
- **Docker** running locally (if `sandbox.provider = "docker"`)
- **AWS account + IAM credentials** (if `sandbox.provider = "ecs"`)
- A **Telegram Bot Token** (get one from [@BotFather](https://t.me/BotFather))
- API keys for at least one agent (e.g. `ANTHROPIC_API_KEY` for Claude Code)

### 1. Clone and build

```bash
git clone https://github.com/manthan787/anycode.git
cd anycode
cargo build --release
```

### 2. Build the sandbox image

```bash
docker build -f docker/Dockerfile.agent -t anycode-sandbox:latest .
```

### 3. Configure

```bash
cp config.example.toml config.toml
```

Edit `config.toml` with your bot token and agent credentials:

```toml
[telegram]
bot_token = "123456:ABC-DEF..."

[agents.credentials.claude-code]
env = { ANTHROPIC_API_KEY = "sk-ant-..." }
```

### 4. Run

```bash
./target/release/anycode --config config.toml
```

### 5. Use it

Open your bot in Telegram and send:

```
/task claude-code fix the login bug in org/repo
```

---

## How It Works

```
You (Telegram)                    Anycode Daemon                     Sandbox Backend
━━━━━━━━━━━━━━                    ━━━━━━━━━━━━━━                     ━━━━━━━━━━━━━━━

/task claude-code                 Parse command
  fix the auth bug ──────────────▶ Check limits
                                  Create sandbox    ──────────────▶  🐳 Docker or ☁️ ECS
                                  Wait for healthy  ◀──────────────  + claude-code
                                  Create session
                                  Send prompt       ──────────────▶  Agent starts working
                                       ◀── SSE stream ────────────
  Streaming output  ◀──────────── Debounced edits
  "Which file?"     ◀──────────── Inline keyboard
                                       │
  [Press button]    ──────────────▶ Reply to agent  ──────────────▶  Agent continues
                                       ◀── SSE stream ────────────
  "Done! Here's     ◀──────────── Final message
   the fix."                      Destroy sandbox   ──────────────▶  🗑️ cleaned up
```

Each session is fully isolated: its own container/task, its own API endpoint, and its own event stream. Sandboxes are automatically destroyed on completion, failure, timeout, or cancellation.

---

## Commands

| Command | Description |
|---------|-------------|
| `/task [agent] <prompt>` | Start a coding task. Agent defaults to config if omitted. |
| `/status` | List active sessions with agent, status, and start time. |
| `/cancel [id]` | Cancel a session. Omit ID to cancel the most recent. |
| `/agents` | List available agents and which is the default. |
| `/help` | Show available commands. |

**Agent selection**: If the first word after `/task` matches a known agent name, it's used as the agent. Otherwise the default agent is used and the full text is the prompt.

```
/task fix the bug               → default agent, prompt = "fix the bug"
/task codex fix the bug         → agent = codex, prompt = "fix the bug"
```

**Repo detection**: GitHub/GitLab URLs (or `org/repo` shorthand) in the prompt are automatically detected and passed to the sandbox.

**Follow-up messages**: Plain text sent while a session is active gets routed to the most recent session in that chat.

**Interactive Q&A**: When the agent asks a question or requests permission, inline buttons appear. Press a button to respond.

---

## Configuration

```toml
[telegram]
bot_token = "YOUR_BOT_TOKEN"           # Required
allowed_users = []                      # Telegram user IDs (empty = allow all)

[sandbox]
provider = "docker"                     # "docker" or "ecs"

[docker]
image = "anycode-sandbox:latest"        # Sandbox container image
port_range_start = 12000                # Host port range for containers
port_range_end = 12100
network = "bridge"

[ecs]
cluster = "anycode-cluster"             # Required when provider = "ecs"
task_definition = "anycode-task:1"      # Required when provider = "ecs"
subnets = ["subnet-abc123"]             # Required when provider = "ecs"
security_groups = ["sg-abc123"]
assign_public_ip = true
container_port = 2468
startup_timeout_secs = 120
poll_interval_ms = 1000
region = "us-west-2"
platform_version = "LATEST"
container_name = "anycode-sandbox"      # Optional; inferred from task def if empty
log_group = "/ecs/anycode"              # Optional, for get_logs
log_stream_prefix = "anycode"           # Optional

[database]
path = "anycode.db"                     # SQLite database file

[agents]
default_agent = "claude-code"           # Default when /task has no agent name

[agents.credentials.claude-code]
env = { ANTHROPIC_API_KEY = "sk-ant-..." }

[agents.credentials.codex]
env = { OPENAI_API_KEY = "sk-..." }

[agents.credentials.goose]
env = { OPENAI_API_KEY = "sk-..." }

[session]
max_concurrent = 5                      # Max active sessions per chat
timeout_minutes = 30                    # Auto-cancel after this duration
debounce_ms = 500                       # Streaming output flush interval
```

Agent credentials are injected as environment variables into the sandbox at creation time. They are never baked into images. Keep `config.toml` out of version control.

### ECS Fargate Notes

- Anycode launches one Fargate task per `/task` via `RunTask`.
- It waits for task state `RUNNING`, resolves the ENI IP, then connects to the sandbox agent on `ecs.container_port`.
- `ANYCODE_AGENT`, `ANYCODE_REPO`, and agent credentials are passed as ECS container environment overrides.
- `ecs.container_name` is optional. If omitted, Anycode infers it from the ECS task definition.
- `get_logs` uses CloudWatch when `ecs.log_group` is configured.

---

## Architecture

```
anycode/
├── crates/
│   ├── anycode-core/           Library: all business logic
│   │   ├── config.rs           TOML config parsing + validation
│   │   ├── error.rs            Unified error types (thiserror)
│   │   ├── db/                 SQLite persistence (tokio-rusqlite)
│   │   ├── messaging/          MessagingProvider trait + Telegram impl
│   │   ├── infra/              SandboxProvider trait + Docker/ECS impls
│   │   ├── sandbox/            HTTP client + SSE stream for sandbox agent
│   │   ├── control/            Telegram ↔ Sandbox bridge orchestration
│   │   └── session/            Timeout watchdog + orphan cleanup
│   └── anycode-bin/            CLI entrypoint (clap + tracing)
├── migrations/                 SQLite schema
├── docker/                     Sandbox container image
└── config.example.toml
```

### Trait abstractions

The two core extension points are traits, making it straightforward to add new messaging platforms or infrastructure backends:

**`MessagingProvider`** &mdash; send/edit messages, handle callbacks, subscribe to events, upload files.
Currently implemented for Telegram. Extensible to Slack, Discord, Matrix.

**`SandboxProvider`** &mdash; create/destroy sandboxes, health check, fetch logs.
Currently implemented for Docker and AWS ECS Fargate. Extensible to Kubernetes and other backends.

### Concurrency model

- **tokio** async runtime with spawned tasks per event, per session, per SSE stream
- **DashMap** for lock-free concurrent session routing
- **Async Mutex** for per-session delta buffers
- **Watch channel** for graceful shutdown broadcast

### Streaming output

Agent output arrives as many small SSE `item.delta` events. Sending each one as a separate Telegram message would hit rate limits and be unreadable. Instead, a **DeltaBuffer** accumulates text and flushes it as a Telegram message edit every 500ms (configurable). When a message approaches Telegram's 4096-char limit, a new message is started automatically.

### Resilience

- **SSE reconnection**: Exponential backoff (1s → 30s cap), max 5 retries
- **Session timeouts**: Background watchdog every 60s
- **Orphan cleanup**: Dead containers detected and failed on startup
- **Graceful shutdown**: SIGTERM destroys all active containers

---

## Sandbox Image

The default sandbox image (`docker/Dockerfile.agent`) is Ubuntu 24.04 with:

- [Rivet Sandbox Agent](https://github.com/nichochar/open-agent-platform) SDK
- [Claude Code](https://www.npmjs.com/package/@anthropic-ai/claude-code) CLI
- [Codex](https://www.npmjs.com/package/@openai/codex) CLI
- Node.js 22, Python 3, git, build-essential

Build it with:

```bash
docker build -f docker/Dockerfile.agent -t anycode-sandbox:latest .
```

The image exposes port `2468` (sandbox agent HTTP API). Each container gets a unique host port from the configured range, mapped to `127.0.0.1` only.

---

## Development

```bash
# Run tests
cargo test

# Run with debug logging
RUST_LOG=debug cargo run -- --config config.toml

# Check compilation
cargo check
```

### Tests

33 unit tests covering config validation (including ECS), database CRUD, bridge behavior, message splitting, URL extraction, delta buffering, and infra helpers. All tests use in-memory SQLite and are fully isolated.

---

## Roadmap

- [ ] Slack and Discord messaging providers
- [ ] Kubernetes sandbox provider
- [ ] ACP (JSON-RPC) protocol support alongside OpenCode REST
- [ ] Git repo cloning into sandbox (private repos via token)
- [ ] File output as Telegram document uploads
- [ ] Per-user rate limiting
- [ ] Web dashboard for session monitoring

---

## License

MIT

---

<p align="center">
  Built with Rust, tokio, teloxide, and bollard.
</p>

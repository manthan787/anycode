# Contributing to Anycode

Thanks for your interest in contributing! Here's how to get started.

## Development Setup

1. **Rust 1.75+** &mdash; install via [rustup](https://rustup.rs/)
2. **Docker** &mdash; running locally for integration testing
3. Clone the repo and build:

```bash
git clone https://github.com/manthan787/anycode.git
cd anycode
cargo build
cargo test
```

## Project Structure

Anycode is a Cargo workspace with two crates:

- **`anycode-core`** &mdash; library crate with all business logic (config, DB, messaging, infra, sandbox, control, session)
- **`anycode-bin`** &mdash; binary crate with CLI entrypoint

All traits, types, and implementations live in `anycode-core`. The binary just wires things together and starts the daemon.

See [agents.md](agents.md) for detailed architecture documentation.

## Adding a New Messaging Provider

1. Create `crates/anycode-core/src/messaging/your_provider.rs`
2. Implement the `MessagingProvider` trait
3. Re-export from `messaging/mod.rs`
4. Add config section and wire it up in `main.rs`

## Adding a New Sandbox Provider

1. Create `crates/anycode-core/src/infra/your_provider.rs`
2. Implement the `SandboxProvider` trait
3. Re-export from `infra/mod.rs`
4. Add config section and wire it up in `main.rs`

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and fix warnings
- Add tests for new functionality
- Keep error handling explicit &mdash; use `Result<T>`, avoid `.unwrap()` in non-test code

## Commit Messages

Use descriptive commit messages. Start with a verb in imperative form:

```
Add Slack messaging provider
Fix SSE reconnection on timeout
Update Docker provider to use builder API
```

## Pull Requests

1. Fork the repo and create a feature branch
2. Make your changes with tests
3. Ensure `cargo test` and `cargo clippy` pass
4. Open a PR with a clear description of what changed and why

## Reporting Issues

Open an issue on GitHub with:

- What you expected to happen
- What actually happened
- Steps to reproduce
- Rust version, OS, Docker version

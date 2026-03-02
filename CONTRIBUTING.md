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

Anycode is a Cargo workspace with three crates:

- **`anycode-core`** &mdash; library crate with all business logic (config, DB, messaging, infra, sandbox, control, session)
- **`anycode-bin`** &mdash; binary crate with CLI entrypoint
- **`anycode-setup`** &mdash; interactive TUI setup wizard (ratatui/crossterm, no dependency on `anycode-core`)

All traits, types, and implementations live in `anycode-core`. The binary just wires things together and starts the daemon. The setup wizard is a standalone tool that generates `config.toml` and runs the build.

See [agents.md](agents.md) for detailed architecture documentation.

## Adding a New Messaging Provider

1. Create `crates/anycode-core/src/messaging/your_provider.rs`
2. Implement the `MessagingProvider` trait (see `traits.rs` for the interface)
3. Re-export from `messaging/mod.rs`
4. Add config section in `config.rs` and wire it up in `main.rs`

See existing implementations in `telegram.rs` and `slack.rs` for reference.

## Adding a New Sandbox Provider

1. Create `crates/anycode-core/src/infra/your_provider.rs`
2. Implement the `SandboxProvider` trait
3. Add a variant to `AnySandboxProvider` in `infra/provider.rs`
4. Re-export from `infra/mod.rs`
5. Add config section in `config.rs` and wire it up in `main.rs`

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

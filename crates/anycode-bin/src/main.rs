use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use anycode_core::config::AppConfig;
use anycode_core::control::bridge::Bridge;
use anycode_core::db::Repository;
use anycode_core::infra::docker::DockerProvider;
use anycode_core::messaging::telegram::TelegramProvider;
use anycode_core::messaging::slack::SlackProvider;
use anycode_core::messaging::MessagingProvider;
use anycode_core::session::manager::SessionWatchdog;

#[derive(Parser)]
#[command(name = "anycode", about = "Run coding agents from Telegram and Slack")]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    config: std::path::PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    info!("Loading config from {:?}", cli.config);
    let config =
        AppConfig::load(&cli.config).context("Failed to load config")?;

    // Initialize database
    info!("Initializing database at {}", config.database.path);
    let repo = Repository::new(&config.database.path)
        .await
        .context("Failed to initialize database")?;

    // Initialize Docker provider
    let docker_provider = DockerProvider::new(
        &config.docker.image,
        &config.docker.network,
        config.docker.port_range_start,
        config.docker.port_range_end,
    )
    .context("Failed to connect to Docker")?;
    let docker_provider = Arc::new(docker_provider);

    // Shutdown signal
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Start watchdog
    let mut watchdog = SessionWatchdog::new(
        config.clone(),
        repo.clone(),
        Arc::clone(&docker_provider),
        shutdown_rx,
    );
    let watchdog_handle = tokio::spawn(async move {
        watchdog.run().await;
    });

    // Build bridges for each configured platform
    let mut bridges: Vec<Arc<Bridge<DockerProvider>>> = Vec::new();

    if let Some(ref tg_config) = config.telegram {
        info!("Initializing Telegram provider");
        let telegram: Arc<dyn MessagingProvider> =
            Arc::new(TelegramProvider::new(&tg_config.bot_token));
        let bridge = Arc::new(Bridge::new(
            config.clone(),
            telegram,
            Arc::clone(&docker_provider),
            repo.clone(),
            tg_config.allowed_users.clone(),
        ));
        bridges.push(bridge);
    }

    if let Some(ref slack_config) = config.slack {
        info!("Initializing Slack provider");
        let slack: Arc<dyn MessagingProvider> =
            Arc::new(SlackProvider::new(&slack_config.app_token, &slack_config.bot_token));
        let bridge = Arc::new(Bridge::new(
            config.clone(),
            slack,
            Arc::clone(&docker_provider),
            repo.clone(),
            slack_config.allowed_users.clone(),
        ));
        bridges.push(bridge);
    }

    if bridges.is_empty() {
        anyhow::bail!("No messaging platforms configured");
    }

    // Handle SIGTERM/SIGINT for graceful shutdown
    let shutdown_bridges: Vec<_> = bridges.iter().map(Arc::clone).collect();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for ctrl_c");
        info!("Received shutdown signal");
        let _ = shutdown_tx.send(true);
        for bridge in &shutdown_bridges {
            if let Err(e) = bridge.shutdown().await {
                error!("Error during shutdown: {e}");
            }
        }
    });

    // Spawn all bridges concurrently
    let mut join_set = tokio::task::JoinSet::new();
    for bridge in bridges {
        join_set.spawn(async move {
            if let Err(e) = bridge.run().await {
                error!("Bridge error: {e}");
            }
        });
    }

    info!("Anycode daemon started");

    // Wait for all bridges to finish
    while let Some(result) = join_set.join_next().await {
        if let Err(e) = result {
            error!("Bridge task panicked: {e}");
        }
    }

    watchdog_handle.abort();
    info!("Anycode daemon stopped");

    Ok(())
}

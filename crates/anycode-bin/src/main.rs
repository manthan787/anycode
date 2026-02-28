use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use anycode_core::config::AppConfig;
use anycode_core::control::bridge::Bridge;
use anycode_core::db::Repository;
use anycode_core::infra::docker::DockerProvider;
use anycode_core::infra::ecs::{EcsFargateProvider, EcsProviderConfig};
use anycode_core::infra::provider::AnySandboxProvider;
use anycode_core::messaging::telegram::TelegramProvider;
use anycode_core::session::manager::SessionWatchdog;

#[derive(Parser)]
#[command(name = "anycode", about = "Run coding agents from Telegram")]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,
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

    let sandbox_provider = Arc::new(
        build_sandbox_provider(&config)
            .await
            .context("Failed to initialize sandbox provider")?,
    );

    // Initialize Telegram provider
    let telegram = TelegramProvider::new(&config.telegram.bot_token);
    let telegram = Arc::new(telegram);

    // Shutdown signal
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Start watchdog
    let mut watchdog = SessionWatchdog::new(
        config.clone(),
        repo.clone(),
        Arc::clone(&sandbox_provider),
        shutdown_rx,
    );
    let watchdog_handle = tokio::spawn(async move {
        watchdog.run().await;
    });

    // Start bridge
    let bridge = Arc::new(Bridge::new(
        config.clone(),
        Arc::clone(&telegram),
        Arc::clone(&sandbox_provider),
        repo.clone(),
    ));

    let bridge_clone = Arc::clone(&bridge);

    // Handle SIGTERM/SIGINT for graceful shutdown
    let shutdown_bridge = Arc::clone(&bridge);
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for ctrl_c");
        info!("Received shutdown signal");
        let _ = shutdown_tx.send(true);
        if let Err(e) = shutdown_bridge.shutdown().await {
            error!("Error during shutdown: {e}");
        }
    });

    info!("Anycode daemon started");
    if let Err(e) = bridge_clone.run().await {
        error!("Bridge error: {e}");
    }

    watchdog_handle.abort();
    info!("Anycode daemon stopped");

    Ok(())
}

async fn build_sandbox_provider(config: &AppConfig) -> anyhow::Result<AnySandboxProvider> {
    match config.sandbox.provider.as_str() {
        "docker" => {
            let provider = DockerProvider::new(
                &config.docker.image,
                &config.docker.network,
                config.docker.port_range_start,
                config.docker.port_range_end,
            )
            .context("Failed to connect to Docker")?;
            Ok(provider.into())
        }
        "ecs" => {
            let ecs_config = EcsProviderConfig {
                cluster: config.ecs.cluster.clone(),
                task_definition: config.ecs.task_definition.clone(),
                subnets: config.ecs.subnets.clone(),
                security_groups: config.ecs.security_groups.clone(),
                assign_public_ip: config.ecs.assign_public_ip,
                container_port: config.ecs.container_port,
                startup_timeout_secs: config.ecs.startup_timeout_secs,
                poll_interval_ms: config.ecs.poll_interval_ms,
                region: config.ecs.region.clone(),
                platform_version: config.ecs.platform_version.clone(),
                container_name: config.ecs.container_name.clone(),
                log_group: config.ecs.log_group.clone(),
                log_stream_prefix: config.ecs.log_stream_prefix.clone(),
            };

            let provider = EcsFargateProvider::new(ecs_config)
                .await
                .context("Failed to initialize ECS provider")?;
            Ok(provider.into())
        }
        other => anyhow::bail!(
            "Unsupported sandbox.provider '{other}'. Expected 'docker' or 'ecs'."
        ),
    }
}

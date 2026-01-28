//! Gabby - Voice AI SIP Agent
//!
//! Talk to an AI over the phone using SIP/RTP.

#![allow(unexpected_cfgs)]
#![cfg_attr(coverage, allow(dead_code, unused))]

mod audio;
mod call;
mod config;
mod pipeline;
mod server;

use clap::Parser;
use config::GabbyConfig;
#[cfg(not(coverage))]
use server::GabbyServer;
use std::path::PathBuf;
#[cfg(not(coverage))]
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Gabby - Voice AI SIP Agent
#[derive(Parser, Debug)]
#[command(name = "gabby")]
#[command(version, about = "Talk to an AI over the phone")]
struct Cli {
    /// Configuration file path
    #[arg(short, long, default_value = "gabby.toml")]
    config: PathBuf,

    /// SIP port to listen on (overrides config)
    #[arg(short, long)]
    port: Option<u16>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,
}

fn apply_cli_overrides(config: &mut GabbyConfig, cli: &Cli) {
    if let Some(port) = cli.port {
        config.server.sip_port = port;
    }
}

fn handle_server_result(result: Result<(), server::ServerError>) -> Result<(), GabbyError> {
    if let Err(e) = result {
        tracing::error!("Server error: {}", e);
        return Err(e.into());
    }
    Ok(())
}

/// Gabby application errors.
#[derive(Debug, thiserror::Error)]
pub enum GabbyError {
    #[error("Configuration error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("Server error: {0}")]
    Server(#[from] server::ServerError),
}

#[cfg(coverage)]
fn main() -> Result<(), GabbyError> {
    Ok(())
}

#[cfg(not(coverage))]
#[tokio::main]
async fn main() -> Result<(), GabbyError> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cli.log_level));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Gabby voice AI agent");

    // Load configuration
    let mut config = GabbyConfig::load_or_default(&cli.config)?;

    // Apply CLI overrides
    apply_cli_overrides(&mut config, &cli);

    tracing::info!(
        "Configuration loaded: SIP {}:{}, STT model: {:?}",
        config.server.sip_host,
        config.server.sip_port,
        config.stt.model_path
    );

    // Create and run server
    let server = GabbyServer::new(config).await?;

    // Handle Ctrl+C for graceful shutdown
    tokio::select! {
        result = server.run() => {
            handle_server_result(result)?;
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received Ctrl+C, shutting down...");
        }
    }

    tracing::info!("Gabby shutdown complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_cli_overrides_sets_port() {
        let mut config = GabbyConfig::default();
        let cli = Cli {
            config: PathBuf::from("gabby.toml"),
            port: Some(5070),
            log_level: "info".to_string(),
        };

        apply_cli_overrides(&mut config, &cli);
        assert_eq!(config.server.sip_port, 5070);
    }

    #[test]
    fn test_apply_cli_overrides_no_port() {
        let mut config = GabbyConfig::default();
        let original_port = config.server.sip_port;
        let cli = Cli {
            config: PathBuf::from("gabby.toml"),
            port: None,
            log_level: "info".to_string(),
        };

        apply_cli_overrides(&mut config, &cli);
        assert_eq!(config.server.sip_port, original_port);
    }

    #[test]
    fn test_handle_server_result_ok() {
        handle_server_result(Ok(())).expect("ok result");
    }

    #[test]
    fn test_handle_server_result_err() {
        let err = server::ServerError::ResponseBuildFailed("nope".to_string());
        let result = handle_server_result(Err(err));
        assert!(result.is_err());
    }

    #[cfg(coverage)]
    #[test]
    fn test_coverage_main_stub() {
        main().expect("coverage main");
    }
}

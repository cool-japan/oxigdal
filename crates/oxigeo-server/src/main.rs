//! OxiGeo Tile Server - Main Entry Point
//!
//! Command-line interface for the tile server.

use clap::Parser;
use oxigeo_server::{Config, TileServer};
use std::path::PathBuf;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// OxiGeo Tile Server
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Configuration file path
    #[arg(short, long, value_name = "FILE", env = "OXIGEO_CONFIG")]
    config: Option<PathBuf>,

    /// Host address to bind to
    #[arg(long, env = "OXIGEO_HOST")]
    host: Option<String>,

    /// Port to bind to
    #[arg(short, long, env = "OXIGEO_PORT")]
    port: Option<u16>,

    /// Number of worker threads
    #[arg(short, long, env = "OXIGEO_WORKERS")]
    workers: Option<usize>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info", env = "OXIGEO_LOG_LEVEL")]
    log_level: String,

    /// Generate a default configuration file
    #[arg(long, value_name = "FILE")]
    generate_config: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    // Parse command-line arguments
    let args = Args::parse();

    // Initialize tracing
    let log_level = args.log_level.parse().unwrap_or(tracing::Level::INFO);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("oxigeo_server={}", log_level).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Handle config generation
    if let Some(output_path) = args.generate_config {
        if let Err(e) = generate_default_config(&output_path) {
            error!("Failed to generate config: {}", e);
            std::process::exit(1);
        }
        info!(
            "Generated default configuration at: {}",
            output_path.display()
        );
        return;
    }

    // Load configuration
    let mut config = if let Some(config_path) = args.config {
        match Config::from_file(&config_path) {
            Ok(config) => {
                info!("Loaded configuration from: {}", config_path.display());
                config
            }
            Err(e) => {
                error!("Failed to load configuration: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        warn!(
            "No configuration file specified (pass --config or set OXIGEO_CONFIG); \
             using built-in defaults, which ignore any mounted server.toml"
        );
        Config::default_config()
    };

    // Apply command-line overrides
    if let Some(host) = args.host {
        if let Ok(addr) = host.parse() {
            config.server.host = addr;
        } else {
            error!("Invalid host address: {}", host);
            std::process::exit(1);
        }
    }

    if let Some(port) = args.port {
        config.server.port = port;
    }

    if let Some(workers) = args.workers {
        config.server.workers = workers;
    }

    // Validate configuration
    if let Err(e) = config.validate() {
        error!("Configuration validation failed: {}", e);
        std::process::exit(1);
    }

    // Create and start server
    let server = match TileServer::new(config) {
        Ok(server) => server,
        Err(e) => {
            error!("Failed to create server: {}", e);
            std::process::exit(1);
        }
    };

    // Run server
    if let Err(e) = server.serve().await {
        error!("Server error: {}", e);
        std::process::exit(1);
    }
}

/// Generate a default configuration file
fn generate_default_config(output_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let default_config = r#"# OxiGeo Tile Server Configuration

[server]
# Host address to bind to
host = "0.0.0.0"

# Port to bind to
port = 8080

# Number of worker threads (0 = number of CPUs)
workers = 0

# Maximum request size in bytes
max_request_size = 10485760  # 10 MB

# Request timeout in seconds
timeout_seconds = 30

# Enable CORS
enable_cors = true

# Allowed CORS origins (empty = all)
cors_origins = []

# Enable the Prometheus /metrics endpoint (served on its own port, see metrics_port)
metrics_enabled = true

# Port the /metrics endpoint listens on
metrics_port = 8081

[cache]
# In-memory cache size in megabytes
memory_size_mb = 256

# Optional disk cache directory
# disk_cache = "/tmp/oxigeo-cache"

# Time-to-live for cached tiles in seconds
ttl_seconds = 3600

# Enable cache statistics
enable_stats = true

# Cache compression (gzip tiles in memory)
compression = false

[metadata]
# Service title
title = "OxiGeo Tile Server"

# Service description
abstract_ = "WMS/WMTS tile server powered by OxiGeo"

# Keywords
keywords = ["tiles", "wms", "wmts", "geospatial"]

# Example layer configuration
# Uncomment and modify for your data
#
# [[layers]]
# name = "landsat"
# title = "Landsat Imagery"
# abstract_ = "Landsat 8 satellite imagery"
# path = "/data/landsat.tif"
# formats = ["png", "jpeg", "webp"]
# tile_size = 256
# min_zoom = 0
# max_zoom = 18
# tile_matrix_sets = ["WebMercatorQuad", "WorldCRS84Quad"]
# enabled = true
#
# # Optional style configuration
# [layers.style]
# name = "default"
# colormap = "viridis"
# value_range = [0.0, 1.0]
# alpha = 1.0
# gamma = 1.0
# brightness = 0.0
# contrast = 1.0
"#;

    std::fs::write(output_path, default_config)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    // Test-only: mutating `OXIGEO_CONFIG` via `std::env::set_var`/`remove_var` (both `unsafe`
    // as of Rust 2024) is required to exercise clap's `env` attribute end-to-end. See the
    // per-test SAFETY comments below.
    #![allow(unsafe_code)]

    use super::Args;
    use clap::Parser;

    /// Regression test for the OXIGEO_CONFIG -> `--config` wiring gap: Dockerfile.server,
    /// docker-compose.yml and k8s/deployment.yaml all set/mount configuration via the
    /// `OXIGEO_CONFIG` environment variable, but never pass `--config` on the CLI, so the
    /// `config` field must populate itself from that env var (matching the existing
    /// host/port/workers/log_level fields).
    #[test]
    fn test_config_arg_reads_oxigeo_config_env_var() {
        // SAFETY: this test is the sole owner of the `OXIGEO_CONFIG` env var name within
        // this test binary; no other test in this crate reads or writes it.
        unsafe {
            std::env::set_var("OXIGEO_CONFIG", "/config/server.toml");
        }

        let args = Args::try_parse_from(["oxigeo-server"]).expect("should parse with no flags");

        unsafe {
            std::env::remove_var("OXIGEO_CONFIG");
        }

        assert_eq!(
            args.config,
            Some(std::path::PathBuf::from("/config/server.toml"))
        );
    }

    #[test]
    fn test_config_arg_flag_overrides_env_var() {
        unsafe {
            std::env::set_var("OXIGEO_CONFIG", "/config/from-env.toml");
        }

        let args = Args::try_parse_from(["oxigeo-server", "--config", "/config/from-flag.toml"])
            .expect("should parse with explicit --config");

        unsafe {
            std::env::remove_var("OXIGEO_CONFIG");
        }

        assert_eq!(
            args.config,
            Some(std::path::PathBuf::from("/config/from-flag.toml"))
        );
    }

    #[test]
    fn test_config_arg_defaults_to_none_without_env_or_flag() {
        unsafe {
            std::env::remove_var("OXIGEO_CONFIG");
        }

        let args = Args::try_parse_from(["oxigeo-server"]).expect("should parse with no flags");
        assert_eq!(args.config, None);
    }
}

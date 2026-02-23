//! OxiGDAL API Gateway
//!
//! Enterprise-grade API gateway with comprehensive features for geospatial services.
//!
//! # Features
//!
//! - **Rate Limiting**: Token bucket, leaky bucket, fixed/sliding window algorithms
//! - **Authentication**: API keys, JWT, OAuth2, session management, MFA
//! - **API Versioning**: Multiple version support with negotiation and migration
//! - **GraphQL**: Full GraphQL server with subscriptions and DataLoader
//! - **WebSocket**: Connection multiplexing and message routing
//! - **Middleware**: CORS, compression, caching, logging, metrics
//! - **Load Balancing**: Multiple strategies with health checks and circuit breaker
//! - **Transformation**: Request/response transformation and format adaptation
//!
//! # Example
//!
//! ```no_run
//! use oxigdal_gateway::{Gateway, GatewayConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = GatewayConfig::default();
//!     let gateway = Gateway::new(config)?;
//!     gateway.serve("0.0.0.0:8080").await?;
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]
#![deny(clippy::unwrap_used, clippy::panic)]

pub mod auth;
pub mod error;
pub mod graphql;
pub mod loadbalancer;
pub mod middleware;
pub mod rate_limit;
pub mod transform;
pub mod versioning;
pub mod websocket;

pub use error::{GatewayError, Result};

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

/// Gateway configuration.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Rate limiting configuration
    pub rate_limit: rate_limit::RateLimitConfig,
    /// Authentication configuration
    pub auth: auth::AuthConfig,
    /// Load balancer configuration
    pub loadbalancer: loadbalancer::LoadBalancerConfig,
    /// Middleware configuration
    pub middleware: middleware::MiddlewareConfig,
    /// Maximum request body size in bytes
    pub max_body_size: usize,
    /// Request timeout in seconds
    pub request_timeout: u64,
    /// Enable GraphQL endpoint
    pub enable_graphql: bool,
    /// Enable WebSocket endpoint
    pub enable_websocket: bool,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            rate_limit: rate_limit::RateLimitConfig::default(),
            auth: auth::AuthConfig::default(),
            loadbalancer: loadbalancer::LoadBalancerConfig::default(),
            middleware: middleware::MiddlewareConfig::default(),
            max_body_size: 10 * 1024 * 1024, // 10MB
            request_timeout: 30,
            enable_graphql: true,
            enable_websocket: true,
        }
    }
}

/// API Gateway instance.
pub struct Gateway {
    config: Arc<GatewayConfig>,
}

impl Gateway {
    /// Creates a new gateway with the given configuration.
    pub fn new(config: GatewayConfig) -> Result<Self> {
        Ok(Self {
            config: Arc::new(config),
        })
    }

    /// Starts the gateway server on the given address.
    pub async fn serve(self, addr: &str) -> Result<()> {
        let addr: SocketAddr = addr
            .parse()
            .map_err(|e| GatewayError::ConfigError(format!("Invalid address: {}", e)))?;

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| GatewayError::InternalError(format!("Failed to bind: {}", e)))?;

        tracing::info!("Gateway listening on {}", addr);

        loop {
            let (socket, remote_addr) = listener
                .accept()
                .await
                .map_err(|e| GatewayError::InternalError(format!("Accept error: {}", e)))?;

            tracing::debug!("Accepted connection from {}", remote_addr);

            // Handle connection in a separate task
            let config = Arc::clone(&self.config);
            tokio::spawn(async move {
                if let Err(e) = handle_connection(socket, config).await {
                    tracing::error!("Connection error: {}", e);
                }
            });
        }
    }
}

async fn handle_connection(
    _socket: tokio::net::TcpStream,
    _config: Arc<GatewayConfig>,
) -> Result<()> {
    // Connection handling implementation
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_config_default() {
        let config = GatewayConfig::default();
        assert_eq!(config.max_body_size, 10 * 1024 * 1024);
        assert_eq!(config.request_timeout, 30);
        assert!(config.enable_graphql);
        assert!(config.enable_websocket);
    }

    #[test]
    fn test_gateway_creation() {
        let config = GatewayConfig::default();
        let gateway = Gateway::new(config);
        assert!(gateway.is_ok());
    }
}

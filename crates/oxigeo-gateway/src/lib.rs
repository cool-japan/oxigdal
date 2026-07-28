//! OxiGeo API Gateway
//!
//! Enterprise-grade API gateway for geospatial services, built on a real axum 0.8 HTTP
//! serving layer that wires together every component below.
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
//! - **Serving Layer**: [`GatewayServer`] binds the above into an axum router
//!
//! # Routes
//!
//! | Method | Path               | Purpose                                            |
//! |--------|--------------------|----------------------------------------------------|
//! | GET    | `/health`          | Liveness plus a per-backend health snapshot        |
//! | GET    | `/gateway/metrics` | Aggregate request/response/error counters (JSON)   |
//! | POST   | `/graphql`         | GraphQL queries and mutations (when enabled)       |
//! | GET    | `/graphql`         | GraphiQL playground (when introspection is enabled) |
//! | *      | `/graphql/ws`      | GraphQL subscriptions (when subscriptions enabled) |
//! | GET    | `/ws`              | WebSocket upgrade (when WebSockets are enabled)     |
//! | *      | *(fallback)*       | Load-balanced reverse proxy to registered backends |
//!
//! # Example
//!
//! ```no_run
//! use oxigeo_gateway::{Gateway, GatewayConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = GatewayConfig::default();
//!     let gateway = Gateway::new(config)?;
//!     gateway.serve("0.0.0.0:8080").await?;
//!     Ok(())
//! }
//! ```
//!
//! For finer control (backends, custom handlers, versioning, transformation), build a
//! [`GatewayServer`] directly:
//!
//! ```no_run
//! use oxigeo_gateway::{GatewayConfig, GatewayServer};
//! use oxigeo_gateway::loadbalancer::Backend;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let server = GatewayServer::builder(GatewayConfig::default())
//!         .with_backend(Backend::new("api".into(), "http://127.0.0.1:9000".into(), 1))
//!         .require_auth(false)
//!         .build()?;
//!     server.serve("0.0.0.0:8080").await?;
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
pub mod server;
pub mod transform;
pub mod versioning;
pub mod websocket;

pub use error::{GatewayError, Result};
pub use server::{GatewayServer, GatewayServerBuilder};

use std::sync::Arc;

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
    ///
    /// Delegates to the axum-based [`GatewayServer`], which serves the full route table
    /// (health, metrics, GraphQL, WebSocket and the load-balanced reverse-proxy fallback)
    /// with the configured rate limiting, authentication and middleware. Serves until a
    /// shutdown signal (ctrl-c / SIGTERM) is received.
    pub async fn serve(self, addr: &str) -> Result<()> {
        GatewayServer::builder((*self.config).clone())
            .build()?
            .serve(addr)
            .await
    }
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

//! Axum-based HTTP serving layer for the OxiGeo API gateway.
//!
//! This module turns the gateway's fully-implemented building blocks (rate limiting,
//! authentication + RBAC, the in-house middleware chain, GraphQL, WebSocket routing, API
//! version negotiation and load-balanced reverse proxying) into a running HTTP service.
//!
//! The entry points are [`GatewayServer`] and its [`GatewayServerBuilder`]. A server is
//! assembled from a [`GatewayConfig`] plus optional backends,
//! handlers and components, then either bound to an address with [`GatewayServer::serve`]
//! or driven directly through its [`router`](GatewayServer::router) (handy for in-process
//! tests via `tower::ServiceExt::oneshot`).
//!
//! # Route table
//!
//! | Method | Path               | Purpose                                             |
//! |--------|--------------------|-----------------------------------------------------|
//! | GET    | `/health`          | Liveness plus a per-backend health snapshot         |
//! | GET    | `/gateway/metrics` | Aggregate request/response/error counters (JSON)    |
//! | POST   | `/graphql`         | GraphQL queries and mutations (when enabled)        |
//! | GET    | `/graphql`         | GraphiQL playground (when introspection is enabled)  |
//! | *      | `/graphql/ws`      | GraphQL subscriptions (when subscriptions enabled)  |
//! | GET    | `/ws`              | WebSocket upgrade (when WebSockets are enabled)      |
//! | *      | *(fallback)*       | Load-balanced reverse proxy to registered backends  |
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use oxigeo_gateway::{GatewayConfig, GatewayServer};
//! use oxigeo_gateway::loadbalancer::Backend;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let server = GatewayServer::builder(GatewayConfig::default())
//!     .with_backend(Backend::new("api".into(), "http://127.0.0.1:9000".into(), 1))
//!     .build()?;
//! server.serve("0.0.0.0:8080").await?;
//! # Ok(())
//! # }
//! ```

mod auth_layer;
mod error_response;
mod graphql;
mod middleware_bridge;
mod proxy;
mod rate_limit_layer;
mod router;
mod state;
mod versioning_layer;
mod ws;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;

use crate::GatewayConfig;
use crate::error::{GatewayError, Result};
use crate::graphql::GraphQLConfig;
use crate::loadbalancer::Backend;
use crate::rate_limit::RateLimiter;
use crate::transform::TransformEngine;
use crate::versioning::deprecation::DeprecationManager;
use crate::versioning::negotiation::VersionNegotiator;
use crate::websocket::WebSocketConfig;
use crate::websocket::router::MessageHandler;

pub(crate) use state::{BuilderOptions, GatewayState};

/// Route-group authorization guard exported as public API.
///
/// Produces an axum layer that rejects requests lacking the given permission with `403`
/// (or `401` when unauthenticated). It is not applied to any built-in route by default;
/// embedders attach it per route group.
pub use auth_layer::require_permission;

/// A fully assembled, ready-to-serve gateway.
///
/// Construct one with [`GatewayServer::builder`]. The server owns its `GatewayState`,
/// which is cheap to clone and shared with every route and layer.
pub struct GatewayServer {
    /// The shared serving-layer state threaded through all routes and middleware.
    state: GatewayState,
}

impl GatewayServer {
    /// Starts a builder for a server with the given configuration.
    pub fn builder(config: GatewayConfig) -> GatewayServerBuilder {
        GatewayServerBuilder {
            config,
            options: BuilderOptions::default(),
        }
    }

    /// Returns the axum [`Router`](axum::Router) for this server.
    ///
    /// Used both by [`serve`](Self::serve) and directly by tests through
    /// `tower::ServiceExt::oneshot`.
    pub fn router(&self) -> axum::Router {
        router::build_router(self.state.clone())
    }

    /// Binds `addr` and serves until a shutdown signal (ctrl-c / SIGTERM) arrives.
    pub async fn serve(self, addr: &str) -> Result<()> {
        let socket_addr: SocketAddr = addr.parse().map_err(|e| {
            GatewayError::ConfigError(format!("invalid listen address '{addr}': {e}"))
        })?;
        let listener = TcpListener::bind(socket_addr).await.map_err(|e| {
            GatewayError::InternalError(format!("failed to bind {socket_addr}: {e}"))
        })?;
        tracing::info!("gateway listening on {socket_addr}");
        self.serve_with_listener(listener).await
    }

    /// Serves on an already-bound listener (used by ephemeral-port integration tests).
    pub async fn serve_with_listener(self, listener: TcpListener) -> Result<()> {
        self.spawn_housekeeping().await;

        let service = router::build_router(self.state.clone())
            .into_make_service_with_connect_info::<SocketAddr>();

        axum::serve(listener, service)
            .with_graceful_shutdown(router::shutdown_signal())
            .await
            .map_err(|e| GatewayError::InternalError(format!("gateway server error: {e}")))?;

        Ok(())
    }

    /// Spawns the background housekeeping tasks before serving begins.
    ///
    /// Health checks are only started when at least one backend is registered (the probe
    /// loop is otherwise a no-op). A detached task periodically evicts idle WebSocket
    /// connections using the configured connection timeout.
    async fn spawn_housekeeping(&self) {
        if !self.state.load_balancer.get_backends().is_empty() {
            self.state.load_balancer.start_health_checks().await;
        }

        let ws_manager = Arc::clone(&self.state.ws_manager);
        let timeout_seconds = self.state.ws_config.connection_timeout as i64;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            // The first tick fires immediately; skip it so we do not sweep before any
            // connection could have gone idle.
            interval.tick().await;
            loop {
                interval.tick().await;
                let removed = ws_manager.cleanup_inactive(timeout_seconds);
                if removed > 0 {
                    tracing::debug!("evicted {removed} idle websocket connection(s)");
                }
            }
        });
    }
}

/// Builder that gathers configuration and optional components for a [`GatewayServer`].
///
/// Every `with_*` method consumes and returns `self` so calls can be chained; [`build`]
/// finalizes the server (and is the only fallible step).
///
/// [`build`]: GatewayServerBuilder::build
pub struct GatewayServerBuilder {
    /// The base gateway configuration.
    config: GatewayConfig,
    /// Accumulated options that cannot be derived from the config alone.
    options: BuilderOptions,
}

impl GatewayServerBuilder {
    /// Registers an upstream backend for the reverse-proxy fallback.
    pub fn with_backend(mut self, backend: Backend) -> Self {
        self.options.backends.push(backend);
        self
    }

    /// Sets whether authentication is mandatory for every non-`/health` route.
    pub fn require_auth(mut self, required: bool) -> Self {
        self.options.require_auth = required;
        self
    }

    /// Declares the direct-peer addresses that are trusted to set `X-Forwarded-For`.
    ///
    /// By default no peer is trusted, so the client IP used for rate limiting and forwarded
    /// upstream is always the direct TCP peer's address and any inbound `X-Forwarded-For` header
    /// is ignored (it is client-controlled and spoofable). List the addresses of your front-end
    /// load balancers / reverse proxies here to honour the client address they forward.
    pub fn with_trusted_proxies(mut self, proxies: Vec<IpAddr>) -> Self {
        self.options.trusted_proxies = proxies;
        self
    }

    /// Overrides the config-derived rate limiter with a caller-supplied one.
    pub fn with_rate_limiter(mut self, limiter: Arc<dyn RateLimiter>) -> Self {
        self.options.rate_limiter = Some(limiter);
        self
    }

    /// Sets the GraphQL configuration (schema limits and route mounting flags).
    pub fn with_graphql_config(mut self, config: GraphQLConfig) -> Self {
        self.options.graphql_config = config;
        self
    }

    /// Sets the WebSocket configuration.
    pub fn with_ws_config(mut self, config: WebSocketConfig) -> Self {
        self.options.ws_config = config;
        self
    }

    /// Registers a WebSocket message handler for a route.
    ///
    /// Registering a handler under the literal route `"default"` replaces the built-in echo
    /// fallback the server would otherwise install.
    pub fn with_ws_handler(
        mut self,
        route: impl Into<String>,
        handler: Arc<dyn MessageHandler>,
    ) -> Self {
        self.options.ws_handlers.push((route.into(), handler));
        self
    }

    /// Sets the API version negotiator.
    pub fn with_version_negotiator(mut self, negotiator: VersionNegotiator) -> Self {
        self.options.version_negotiator = Some(negotiator);
        self
    }

    /// Sets the deprecation policy manager.
    pub fn with_deprecation_manager(mut self, manager: DeprecationManager) -> Self {
        self.options.deprecations = Some(manager);
        self
    }

    /// Sets the request transformation engine.
    pub fn with_transform_engine(mut self, engine: TransformEngine) -> Self {
        self.options.transform = Some(engine);
        self
    }

    /// Builds the server, constructing the shared state.
    ///
    /// Fails with [`GatewayError::ConfigError`] when `require_auth` is set but no
    /// authentication method can be constructed.
    pub fn build(self) -> Result<GatewayServer> {
        let state = GatewayState::build(self.config, self.options)?;
        Ok(GatewayServer { state })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn builder_builds_default_server() {
        let server = GatewayServer::builder(GatewayConfig::default()).build();
        assert!(server.is_ok());
    }

    #[test]
    fn builder_registers_backends() {
        let server = GatewayServer::builder(GatewayConfig::default())
            .with_backend(Backend::new(
                "b1".to_string(),
                "http://127.0.0.1:1".to_string(),
                1,
            ))
            .build()
            .expect("build must succeed");
        assert_eq!(server.state.load_balancer.get_backends().len(), 1);
    }

    #[test]
    fn router_is_constructible() {
        let server = GatewayServer::builder(GatewayConfig::default())
            .build()
            .expect("build must succeed");
        // Constructing the router must not panic and yields a usable axum Router.
        let _router = server.router();
    }

    #[test]
    fn require_auth_without_method_fails_to_build() {
        let mut config = GatewayConfig::default();
        config.auth.enable_api_key = false;
        config.auth.enable_jwt = false;
        config.auth.enable_oauth2 = false;
        config.auth.enable_session = false;
        let result = GatewayServer::builder(config).require_auth(true).build();
        assert!(matches!(result, Err(GatewayError::ConfigError(_))));
    }
}

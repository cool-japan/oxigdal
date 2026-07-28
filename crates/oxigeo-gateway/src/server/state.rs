//! Shared serving-layer state and its construction from a [`GatewayConfig`].
//!
//! [`GatewayState`] bundles every runtime component the axum routes and layers need
//! (rate limiter, authenticator, RBAC, load balancer, failover, the in-house middleware
//! chain, the GraphQL schema, the WebSocket manager, version negotiation and the optional
//! transform engine). It is cheap to clone because every heavyweight member is behind an
//! [`Arc`]. [`GatewayState::build`] turns a config plus the builder-collected
//! [`BuilderOptions`] into a ready-to-serve state, failing fast on inconsistent
//! authentication configuration.

use crate::GatewayConfig;
use crate::auth::MultiAuthenticator;
use crate::auth::rbac::RbacManager;
use crate::error::{GatewayError, Result};
use crate::graphql::{GraphQLConfig, GraphQLSchema, create_schema};
use crate::loadbalancer::advanced::{FailoverConfig, FailoverManager};
use crate::loadbalancer::{Backend, LoadBalancer};
use crate::middleware::caching::CachingMiddleware;
use crate::middleware::metrics::MetricsMiddleware;
use crate::middleware::{MiddlewareChain, compression, cors, logging};
use crate::rate_limit::{FixedWindow, MemoryStorage, RateLimiter, StandardRateLimiter};
use crate::transform::TransformEngine;
use crate::versioning::VersionNegotiator;
use crate::versioning::deprecation::DeprecationManager;
use crate::websocket::router::{EchoHandler, MessageHandler};
use crate::websocket::{WebSocketConfig, WebSocketManager};
use std::net::IpAddr;
use std::sync::Arc;

/// Shared state for all gateway routes and layers.
///
/// Cheap to clone: every field is either a small value or an [`Arc`]. A fresh clone is
/// handed to each axum handler (via `State`) and to each `from_fn_with_state` layer.
#[derive(Clone)]
pub struct GatewayState {
    /// The gateway configuration this state was built from.
    pub config: Arc<GatewayConfig>,
    /// Whether authentication is mandatory for non-`/health` routes.
    pub require_auth: bool,
    /// Peer addresses trusted to set `X-Forwarded-For`.
    ///
    /// A forwarded client IP is only honoured when the direct TCP peer is one of these addresses;
    /// otherwise the peer's own address is used. This closes the spoofable-`X-Forwarded-For`
    /// trust boundary for rate-limit keying and upstream forwarding.
    pub trusted_proxies: Arc<Vec<IpAddr>>,
    /// Rate limiter, present when rate limiting is enabled (or a builder override was set).
    pub rate_limiter: Option<Arc<dyn RateLimiter>>,
    /// Multi-method authenticator, present when at least one auth method is configured.
    pub authenticator: Option<Arc<MultiAuthenticator>>,
    /// Role-based access control manager, used by `require_permission`.
    pub rbac: Option<Arc<RbacManager>>,
    /// Load balancer that selects backends for the reverse proxy fallback.
    pub load_balancer: Arc<LoadBalancer>,
    /// Failover manager that retries proxy attempts across backends.
    pub failover: Arc<FailoverManager>,
    /// In-house middleware chain (CORS/compression/caching/logging/metrics).
    pub middleware_chain: Arc<MiddlewareChain>,
    /// Metrics middleware, kept separately so `/gateway/metrics` can read its counters.
    pub metrics: Option<Arc<MetricsMiddleware>>,
    /// Caching middleware, kept separately for the serving layer's cache short-circuit.
    pub caching: Option<Arc<CachingMiddleware>>,
    /// GraphQL schema, `Some` iff `config.enable_graphql`.
    pub graphql: Option<GraphQLSchema>,
    /// GraphQL configuration governing introspection/subscription route mounting.
    pub graphql_config: GraphQLConfig,
    /// WebSocket connection manager and message router.
    pub ws_manager: Arc<WebSocketManager>,
    /// WebSocket configuration (message size, keepalive, per-user cap, timeout).
    pub ws_config: WebSocketConfig,
    /// Version negotiator, present when API versioning is configured.
    pub version_negotiator: Option<Arc<VersionNegotiator>>,
    /// Deprecation manager, present when deprecation policies are configured.
    pub deprecations: Option<Arc<DeprecationManager>>,
    /// Optional request transformation engine applied to upstream-bound requests.
    pub transform: Option<Arc<TransformEngine>>,
}

/// Plain options struct filled by `GatewayServerBuilder` before [`GatewayState::build`].
///
/// Crate-private: only the builder writes it and only [`GatewayState::build`] reads it.
#[derive(Default)]
pub(crate) struct BuilderOptions {
    /// Backends registered on the load balancer.
    pub(crate) backends: Vec<Backend>,
    /// Whether authentication is required.
    pub(crate) require_auth: bool,
    /// Peer addresses trusted to set `X-Forwarded-For`.
    pub(crate) trusted_proxies: Vec<IpAddr>,
    /// Optional caller-supplied rate limiter overriding the config-derived one.
    pub(crate) rate_limiter: Option<Arc<dyn RateLimiter>>,
    /// GraphQL configuration.
    pub(crate) graphql_config: GraphQLConfig,
    /// WebSocket configuration.
    pub(crate) ws_config: WebSocketConfig,
    /// WebSocket message handlers keyed by route.
    pub(crate) ws_handlers: Vec<(String, Arc<dyn MessageHandler>)>,
    /// Optional version negotiator.
    pub(crate) version_negotiator: Option<VersionNegotiator>,
    /// Optional deprecation manager.
    pub(crate) deprecations: Option<DeprecationManager>,
    /// Optional request transform engine.
    pub(crate) transform: Option<TransformEngine>,
}

impl GatewayState {
    /// Builds state from config plus builder options.
    ///
    /// Fails with [`GatewayError::ConfigError`] when `require_auth` is set but no
    /// authentication method can be constructed (all disabled, or JWT-only with no secret).
    pub(crate) fn build(config: GatewayConfig, opts: BuilderOptions) -> Result<Self> {
        // Rate limiter: a builder override wins; otherwise derive a fixed-window limiter
        // from the config when rate limiting is enabled with a positive request budget.
        let rate_limiter: Option<Arc<dyn RateLimiter>> = if let Some(limiter) = opts.rate_limiter {
            Some(limiter)
        } else if config.rate_limit.enabled && config.rate_limit.max_requests > 0 {
            Some(Arc::new(StandardRateLimiter::new(
                MemoryStorage::new(),
                FixedWindow,
                config.rate_limit.max_requests,
                config.rate_limit.window,
            )))
        } else {
            None
        };

        // Authenticator: disable JWT (with a warning) when no secret is configured, then
        // build a MultiAuthenticator only if at least one method survives.
        let mut auth_config = config.auth.clone();
        if auth_config.enable_jwt && auth_config.jwt_secret.is_none() {
            auth_config.enable_jwt = false;
            tracing::warn!(
                "JWT authentication was requested without a configured secret; disabling JWT"
            );
        }
        let any_method = auth_config.enable_api_key
            || auth_config.enable_jwt
            || auth_config.enable_oauth2
            || auth_config.enable_session;
        let authenticator = if any_method {
            Some(Arc::new(MultiAuthenticator::from_config(&auth_config)?))
        } else {
            None
        };
        if opts.require_auth && authenticator.is_none() {
            return Err(GatewayError::ConfigError(
                "authentication required but no auth method is configured".to_string(),
            ));
        }

        // RBAC is always available (cheap) so `require_permission` can perform lookups.
        let rbac = Some(Arc::new(RbacManager::new()));

        // Load balancer + registered backends.
        let load_balancer = Arc::new(LoadBalancer::new(config.loadbalancer.clone()));
        for backend in opts.backends {
            load_balancer.add_backend(backend);
        }

        // Failover manager: wiring `retry_attempts` here makes the previously-ignored config
        // field real for the reverse-proxy path.
        let failover = Arc::new(FailoverManager::new(FailoverConfig {
            max_retries: config.loadbalancer.retry_attempts,
            ..Default::default()
        }));

        // In-house middleware chain, built manually so the metrics/caching Arcs can also be
        // held in state. Order mirrors MiddlewareManager::from_config exactly.
        let mut chain = MiddlewareChain::new();
        if config.middleware.enable_cors {
            chain.add(Arc::new(cors::CorsMiddleware::new(
                config.middleware.cors.clone(),
            )));
        }
        if config.middleware.enable_compression {
            chain.add(Arc::new(compression::CompressionMiddleware::new(
                config.middleware.compression.clone(),
            )));
        }
        let caching = if config.middleware.enable_caching {
            let middleware = Arc::new(CachingMiddleware::new(config.middleware.cache.clone()));
            chain.add(middleware.clone());
            Some(middleware)
        } else {
            None
        };
        if config.middleware.enable_logging {
            chain.add(Arc::new(logging::LoggingMiddleware::new()));
        }
        let metrics = if config.middleware.enable_metrics {
            let middleware = Arc::new(MetricsMiddleware::new());
            chain.add(middleware.clone());
            Some(middleware)
        } else {
            None
        };
        let middleware_chain = Arc::new(chain);

        // GraphQL schema only when enabled.
        let graphql_config = opts.graphql_config;
        let graphql = config
            .enable_graphql
            .then(|| create_schema(graphql_config.clone()));

        // WebSocket manager + handler registration. Register a fallback EchoHandler under the
        // literal "default" route when the caller did not, since the router errors on every
        // message otherwise.
        let ws_manager = Arc::new(WebSocketManager::new());
        let mut has_default = false;
        for (route, handler) in opts.ws_handlers {
            if route == "default" {
                has_default = true;
            }
            ws_manager.router().register_handler(route, handler);
        }
        if !has_default {
            ws_manager
                .router()
                .register_handler("default".to_string(), Arc::new(EchoHandler));
        }

        let version_negotiator = opts.version_negotiator.map(Arc::new);
        let deprecations = opts.deprecations.map(Arc::new);
        let transform = opts.transform.map(Arc::new);

        Ok(Self {
            config: Arc::new(config),
            require_auth: opts.require_auth,
            trusted_proxies: Arc::new(opts.trusted_proxies),
            rate_limiter,
            authenticator,
            rbac,
            load_balancer,
            failover,
            middleware_chain,
            metrics,
            caching,
            graphql,
            graphql_config,
            ws_manager,
            ws_config: opts.ws_config,
            version_negotiator,
            deprecations,
            transform,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_builds_rate_limiter_and_rbac() {
        let state = GatewayState::build(GatewayConfig::default(), BuilderOptions::default())
            .expect("default config must build");
        // Default rate limiting is enabled with a positive budget.
        assert!(state.rate_limiter.is_some());
        // RBAC is always present.
        assert!(state.rbac.is_some());
        // GraphQL is enabled by default.
        assert!(state.graphql.is_some());
    }

    #[test]
    fn test_default_config_keeps_authenticator_via_api_key_and_session() {
        // Default auth enables api_key + jwt + session. JWT is dropped (no secret), but
        // api_key and session remain, so a MultiAuthenticator is still constructed.
        let state = GatewayState::build(GatewayConfig::default(), BuilderOptions::default())
            .expect("default config must build");
        assert!(state.authenticator.is_some());
    }

    #[test]
    fn test_jwt_only_without_secret_yields_no_authenticator() {
        let mut config = GatewayConfig::default();
        config.auth.enable_api_key = false;
        config.auth.enable_jwt = true;
        config.auth.jwt_secret = None;
        config.auth.enable_oauth2 = false;
        config.auth.enable_session = false;

        let state =
            GatewayState::build(config, BuilderOptions::default()).expect("build must succeed");
        // JWT was disabled for lack of a secret and nothing else was enabled.
        assert!(state.authenticator.is_none());
    }

    #[test]
    fn test_require_auth_without_any_method_is_config_error() {
        let mut config = GatewayConfig::default();
        config.auth.enable_api_key = false;
        config.auth.enable_jwt = false;
        config.auth.enable_oauth2 = false;
        config.auth.enable_session = false;

        let opts = BuilderOptions {
            require_auth: true,
            ..Default::default()
        };
        let result = GatewayState::build(config, opts);
        assert!(matches!(result, Err(GatewayError::ConfigError(_))));
    }

    #[test]
    fn test_backend_registration() {
        let opts = BuilderOptions {
            backends: vec![Backend::new(
                "b1".to_string(),
                "http://127.0.0.1:9".to_string(),
                1,
            )],
            ..Default::default()
        };
        let state =
            GatewayState::build(GatewayConfig::default(), opts).expect("build must succeed");
        assert_eq!(state.load_balancer.get_backends().len(), 1);
    }

    #[test]
    fn test_rate_limiter_absent_when_disabled_and_no_override() {
        let mut config = GatewayConfig::default();
        config.rate_limit.enabled = false;
        let state =
            GatewayState::build(config, BuilderOptions::default()).expect("build must succeed");
        assert!(state.rate_limiter.is_none());
    }

    #[test]
    fn test_graphql_absent_when_disabled() {
        let config = GatewayConfig {
            enable_graphql: false,
            ..Default::default()
        };
        let state =
            GatewayState::build(config, BuilderOptions::default()).expect("build must succeed");
        assert!(state.graphql.is_none());
    }
}

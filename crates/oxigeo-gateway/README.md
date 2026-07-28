# OxiGeo Gateway

[![Crates.io](https://img.shields.io/crates/v/oxigeo-gateway.svg)](https://crates.io/crates/oxigeo-gateway)
[![Documentation](https://docs.rs/oxigeo-gateway/badge.svg)](https://docs.rs/oxigeo-gateway)
[![License](https://img.shields.io/crates/l/oxigeo-gateway.svg)](LICENSE)

API gateway for geospatial services, built in **100% Pure Rust**. `oxigeo-gateway`
provides a real [axum](https://github.com/tokio-rs/axum) 0.8 HTTP serving layer that
binds the crate's components — rate limiting, JWT/API-key/session authentication with
RBAC/MFA, a GraphQL endpoint (queries, mutations, subscriptions), WebSocket routing, API
version negotiation, an in-house middleware chain, and a load-balanced reverse proxy — into
one running service via [`GatewayServer`].

## Features

- **HTTP serving layer**: [`GatewayServer`] / [`GatewayServerBuilder`] assemble an axum
  router from a `GatewayConfig` plus optional backends, handlers, and components; serve on
  an address or drive the `Router` directly in-process (handy for `tower` `oneshot` tests)
- **Rate Limiting**: multiple algorithms (token bucket, leaky bucket, fixed/sliding window)
  with memory and distributed Redis backends; the serving layer applies an atomic
  `try_acquire` and emits `X-RateLimit-*` / `Retry-After` headers
- **Authentication**: API keys, JWT, session management, OAuth2/OIDC (optional `oauth2`
  feature), and multi-factor authentication (MFA); the layer authenticates when credentials
  are present and can enforce a `require_auth` mode (`require_mfa` is honored)
- **Authorization**: role-based access control (RBAC) with fine-grained permissions and a
  `require_permission` route-group guard
- **GraphQL**: `async-graphql`-backed endpoint with queries, mutations, and subscriptions
  (subscriptions and the GraphiQL playground are each gated on config flags)
- **WebSocket**: connection multiplexing and message routing with per-user connection caps
  and ping keepalive
- **Middleware chain**: CORS (with real `OPTIONS` preflight), response compression
  (`Accept-Encoding` negotiated, gzip/brotli via `oxiarc`), LRU+TTL response caching,
  structured logging, and metrics collection
- **Load balancing & reverse proxy**: round-robin / least-connections / weighted strategies
  with real health probing, circuit breaking, and `retry_attempts`-driven failover; the
  fallback route streams upstream responses (HTTPS via the Pure-Rust OxiTLS stack)
- **API Versioning**: URL-path / header / query negotiation with deprecation warnings
- **Pure Rust**: no C/C++/Fortran in the default feature closure

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigeo-gateway = "0.2"
tokio = { version = "1", features = ["full"] }
```

### Feature Flags

```toml
# In-memory rate limiting (default)
oxigeo-gateway = { version = "0.2", features = ["memory"] }

# Distributed rate limiting with Redis
oxigeo-gateway = { version = "0.2", features = ["redis"] }
```

The optional `oauth2` feature enables the OAuth2 Authorization Code flow. It is **off by
default** because its `reqwest`/`rustls-tls` backend pulls `ring` (C + assembly crypto),
which would break the Pure-Rust-by-default guarantee; enable it explicitly when you need
that flow.

## Quick Start

### Serve the gateway

```rust
use oxigeo_gateway::{GatewayConfig, GatewayServer};
use oxigeo_gateway::loadbalancer::Backend;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = GatewayServer::builder(GatewayConfig::default())
        .with_backend(Backend::new("api".into(), "http://127.0.0.1:9000".into(), 1))
        .build()?;

    // Binds and serves until ctrl-c / SIGTERM (graceful shutdown).
    server.serve("0.0.0.0:8080").await?;
    Ok(())
}
```

`Gateway::new(config)?.serve(addr).await` remains available and now delegates to the same
axum-based `GatewayServer`.

### Configure auth, rate limiting, and components

```rust
use std::sync::Arc;
use oxigeo_gateway::{GatewayConfig, GatewayServer};
use oxigeo_gateway::auth::AuthConfig;
use oxigeo_gateway::rate_limit::RateLimitConfig;
use oxigeo_gateway::loadbalancer::Backend;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = GatewayConfig::default();

    config.auth = AuthConfig {
        enable_api_key: true,
        enable_jwt: true,
        enable_session: true,
        require_mfa: false,
        jwt_secret: Some("your-secret-key-at-least-32-chars".to_string()),
        jwt_expiration: 3600,
        session_timeout: 1800,
        ..Default::default()
    };
    config.rate_limit = RateLimitConfig { enabled: true, ..Default::default() };
    config.max_body_size = 50 * 1024 * 1024; // 50 MB
    config.request_timeout = 60;             // seconds
    config.enable_graphql = true;
    config.enable_websocket = true;

    let server = GatewayServer::builder(config)
        .require_auth(true) // reject unauthenticated requests (except /health)
        .with_backend(Backend::new("tiles".into(), "https://tiles.internal:8443".into(), 1))
        .build()?;

    server.serve("0.0.0.0:8080").await?;
    Ok(())
}
```

### In-process testing (no socket)

```rust
use oxigeo_gateway::{GatewayConfig, GatewayServer};
use tower::ServiceExt; // for `oneshot`
use axum::body::Body;
use axum::http::Request;

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let router = GatewayServer::builder(GatewayConfig::default()).build()?.router();
let response = router
    .oneshot(Request::builder().uri("/health").body(Body::empty())?)
    .await?;
assert_eq!(response.status(), 200);
# Ok(())
# }
```

## Route Table

| Method | Path               | Purpose                                                      |
|--------|--------------------|--------------------------------------------------------------|
| GET    | `/health`          | Liveness plus a per-backend health snapshot (auth-exempt)    |
| GET    | `/gateway/metrics` | Aggregate request/response/error counters (JSON)             |
| POST   | `/graphql`         | GraphQL queries and mutations (when `enable_graphql`)        |
| GET    | `/graphql`         | GraphiQL playground (only when introspection is enabled)     |
| *      | `/graphql/ws`      | GraphQL subscriptions (only when `enable_subscriptions`)     |
| GET    | `/ws`              | WebSocket upgrade (only when `enable_websocket`)             |
| *      | *(fallback)*       | Load-balanced reverse proxy to registered backends           |

Effective layer order (outermost first): tracing → version negotiation → in-house
middleware chain → authentication → rate limiting → request timeout → body-size limit →
routes / fallback.

## Builder Options

`GatewayServer::builder(config)` returns a `GatewayServerBuilder` with:

| Method | Effect |
|--------|--------|
| `with_backend(Backend)` | Register an upstream for the reverse-proxy fallback and health checks |
| `require_auth(bool)` | Enforce authentication on every route except `/health` |
| `with_rate_limiter(Arc<dyn RateLimiter>)` | Override the config-derived rate limiter |
| `with_graphql_config(GraphQLConfig)` | Configure introspection, subscriptions, depth limits |
| `with_ws_config(WebSocketConfig)` | Set per-user caps, message size, keepalive interval |
| `with_ws_handler(route, Arc<dyn MessageHandler>)` | Register a WebSocket message handler |
| `with_version_negotiator(VersionNegotiator)` | Enable API version negotiation |
| `with_deprecation_manager(DeprecationManager)` | Emit deprecation `Warning` headers |
| `with_transform_engine(TransformEngine)` | Apply request-side transformation before proxying |
| `with_trusted_proxies(Vec<IpAddr>)` | Allowlist for `X-Forwarded-For` handling |
| `build()` | Construct the `GatewayServer` (`Err` if `require_auth` but no auth method is configured) |

The `require_permission("perm")` guard (re-exported at the crate root) produces an axum
layer for RBAC-protected route groups; it is not applied to any built-in route by default.

## Component Modules

| Module | Description |
|--------|-------------|
| `server` | The axum serving layer (`GatewayServer`, `GatewayServerBuilder`, `require_permission`) |
| `auth` | Authentication and authorization (API keys, JWT, OAuth2, sessions, MFA, RBAC) |
| `rate_limit` | Rate limiting with multiple algorithms and storage backends |
| `graphql` | GraphQL schema, context, and configuration |
| `websocket` | WebSocket connection manager, router, and message handlers |
| `middleware` | HTTP middleware chain (CORS, compression, caching, logging, metrics; plus `middleware::advanced`) |
| `loadbalancer` | Backends, strategies, health checks, circuit breaker, and failover |
| `transform` | Request/response transformation engine |
| `versioning` | API version negotiation, migration, and deprecation |
| `error` | `GatewayError` with HTTP status codes, retry semantics, and `IntoResponse` |

## Error Handling

This library follows the "no `unwrap()`" policy — all fallible operations return
`Result<T, GatewayError>`. `GatewayError` implements `axum::response::IntoResponse`, so
errors surfaced by the serving layer become well-formed HTTP responses:

```rust
use oxigeo_gateway::GatewayError;

let error = GatewayError::RateLimitExceeded {
    message: "Quota exceeded".to_string(),
    retry_after: Some(60),
};

assert_eq!(error.status_code(), 429);   // HTTP 429 Too Many Requests
assert!(error.is_retryable());
assert_eq!(error.retry_after(), Some(60)); // -> Retry-After header
```

## Honest Limitations

The serving layer is real and tested, but a few capabilities are deliberately deferred to
a future release (each has a safe, honest behavior today rather than a silent fake):

- **GraphQL resolvers serve demo / in-memory data** — there is no storage backend wired in
  yet; the schema executes with an injected request context but returns example data.
- **Buffered proxy requests and middleware hops** — the in-house middleware chain and the
  reverse proxy buffer request bodies (bounded by `max_body_size`); proxy **responses**
  stream through unbuffered.
- **No WebSocket pass-through proxying** — `/ws` terminates at the gateway's own
  `WebSocketManager`; upstream WebSocket connections are not proxied.
- **No upstream keep-alive pooling** — each proxied request opens a fresh upstream
  connection (`Connection: close` semantics).
- **Response-side transformation is not wired** — the `TransformEngine` is applied to the
  outbound (upstream-bound) request only; response transformation is available via
  `ResponseTransformer` for embedders but is not invoked automatically.

## Pure Rust

The default feature set is 100% Pure Rust with no C/C++/Fortran dependencies. HTTPS health
probes and upstream connections use the Pure-Rust OxiTLS (rustls + RustCrypto) stack — no
`ring`, OpenSSL, or system TLS. The only C-pulling path is the opt-in `oauth2` feature.

## Testing

```bash
# All tests (381 unit/integration + 3 doctests)
cargo test --all-features

# With the Redis backend
cargo test --features redis
```

## Documentation

Full API documentation is available at [docs.rs](https://docs.rs/oxigeo-gateway). Build it
locally with `cargo doc --open`.

## OxiGeo Ecosystem

This crate is part of the OxiGeo ecosystem for geospatial data processing:

- **oxigeo-core** — core geospatial data structures and operations
- **oxigeo-server** — OGC HTTP server (WMS/WFS)
- **oxigeo-security** — encryption, hashing, RBAC/ABAC
- **oxigeo-observability** — metrics, tracing, alerting

## License

Licensed under the Apache License, Version 2.0.

---

Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem of pure Rust geospatial
libraries and tools.
</content>
</invoke>

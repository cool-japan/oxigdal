# TODO: oxigeo-gateway

> **Purpose:** Enterprise API gateway — rate-limiting, JWT/OAuth2 auth, GraphQL, WebSocket multiplexing, load balancing — for OxiGeo services.
> **Status (2026-05-16):** 17,917 LoC (src) · 245 tests (225 inline + 20 in tests/) · 2 real-code stubs
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Implement the actual HTTP request pipeline in `handle_connection` (currently no-op).
  - **Verified gap:** `src/lib.rs:129-135` —
    ```rust
    async fn handle_connection(
        _socket: tokio::net::TcpStream,
        _config: Arc<GatewayConfig>,
    ) -> Result<()> {
        // Connection handling implementation
        Ok(())
    }
    ```
    The gateway accepts TCP connections in `Gateway::serve()` (`lib.rs:99-126`) but **does not parse HTTP, dispatch to handlers, or invoke any of the 225 inline tests' middleware/auth/rate-limit code paths**. The functional surface is presently un-reachable through the binary entry point.
  - **Goal:** `Gateway::serve(addr)` boots a real axum `Router` that composes (in order): TLS terminator (optional, rustls) → request-ID + tracing → CORS → compression → auth (JWT/OAuth2/API key) → rate limiter (token bucket / sliding window per `RateLimitConfig`) → versioning → routing to backends via `LoadBalancer::pick()`. Body size enforced from `config.max_body_size`; request timeout from `config.request_timeout`.
  - **Design:** Replace the raw `TcpListener::accept()` loop with `axum::serve(listener, router).with_graceful_shutdown(...)`. Each existing module (`auth::*`, `rate_limit::*`, `middleware::*`, `loadbalancer::*`) already exposes the data structures; convert each to a `tower::Layer` and stack via `Router::layer(...)`. JWT validation per RFC 7519; OAuth2 per RFC 6749 §4.1 (Auth Code) and §4.4 (Client Credentials); JWKS rotation per RFC 7517 with TTL fetched in background task. Backend health-checks poll via existing `health::HealthChecker`.
  - **Files:** `src/lib.rs:129-135` (rewrite); `src/router.rs` (new — Router/Layer construction); each `src/{auth,rate_limit,middleware}/*` (add `impl Service` or `impl tower::Layer`).
  - **Tests:** (proposed) `test_serve_returns_200_on_healthy_backend`, `test_serve_returns_401_without_jwt`, `test_serve_returns_429_after_rate_limit`, `test_serve_compression_negotiated`, `test_serve_cors_preflight`, `test_serve_jwt_jwks_rotation`, `test_oauth2_authorization_code_flow_pkce`, `test_request_id_propagated_to_backend`.
  - **Risk:** Largest single-piece deliverable in the crate; partition into PRs (router skeleton → auth layer → rate-limit layer → versioning → load-balancer dispatch).
  - **Prerequisites:** None — all sub-systems already exist as standalone data structures.

- [ ] Implement real HTTP backend probing in `LoadBalancer::custom_check` (currently logs and returns healthy).
  - **Verified gap:** `src/loadbalancer/advanced.rs:545-549` — `// Custom check placeholder` followed by `(true, None, None)` constant return; also `grpc_check` at `advanced.rs:538-542` similarly returns `(true, None, None)` with `// Simplified gRPC health check - in production, use tonic`.
  - **Goal:** `custom_check` issues an HTTP HEAD/GET to the backend `health_check_path`, applies `expected_status` predicate, and returns `(healthy, status_code, error_msg)`. `grpc_check` performs a `grpc.health.v1.Health/Check` RPC.
  - **Design:** Reuse the same `reqwest::Client` already constructed for `http_check` (visible at `advanced.rs` upstream). For gRPC, gate behind a `grpc` feature flag — bring in `tonic` only when needed. Timeout enforced via `config.timeout_ms`.
  - **Files:** `src/loadbalancer/advanced.rs:538-549`.
  - **Tests:** (proposed) `test_custom_check_200_marks_healthy`, `test_custom_check_5xx_marks_unhealthy`, `test_custom_check_timeout_marks_unhealthy`, `test_grpc_check_serving_status`.
  - **Risk:** gRPC adds tonic + protobuf compile cost — keep feature-gated.
  - **Prerequisites:** None.

- [ ] JWT validation via JWKS endpoint discovery with background key rotation.
  - **Goal:** `jsonwebtoken::DecodingKey` populated from a remote JWKS document (RFC 7517 §5) on startup and refreshed every `kid_rotation_interval`; `kid` header drives key selection; expired keys evicted.
  - **Design:** `JwksClient { url, cache: DashMap<kid, DecodingKey>, ttl }`. Background tokio task polls `${issuer}/.well-known/jwks.json` (RFC 8414 metadata discovery optional). Verification uses `Validation::new(alg).set_issuer(...).set_audience(...)` per RFC 7519 §4.1.
  - **Files:** `src/auth/jwt.rs` (already 299 LoC of HS256 + RS256 plumbing; add JWKS module).
  - **Tests:** (proposed) `test_jwks_fetch_and_decode`, `test_jwks_rotation_invalidates_stale_kid`, `test_jwks_missing_kid_returns_401`.
  - **Risk:** Time-of-check vs time-of-use — always re-check exp on every request.
  - **Prerequisites:** Request pipeline (item 1).

## Medium Priority
- [ ] OAuth2 Authorization Code Flow with PKCE (RFC 7636).
  - **Goal:** `/oauth/authorize` + `/oauth/token` endpoints with `code_challenge`/`code_verifier` validation.
  - **Files:** `src/auth/oauth2.rs:359 LoC` (already has client-credentials and password grant stubs).
  - **Why deferred:** Browser flow needs UI; server-to-server (client_credentials) is currently the primary use case.

- [ ] API key rotation and revocation persistence (currently in-memory `DashMap`).
  - **Files:** `src/auth/api_key.rs`.
  - **Why deferred:** Pluggable store trait exists; concrete Redis/Postgres impl is downstream.

- [ ] GraphQL query depth limiting + cost analysis (currently `async-graphql` accepts unbounded queries).
  - **Files:** `src/graphql/mod.rs`, depth via `async_graphql::extensions::limit_depth`.
  - **Why deferred:** Schema is intentionally small in v0.1.

- [ ] WebSocket upgrade and backend forwarding (multiplexer exists; tunnel doesn't yet).
  - **Files:** `src/websocket/multiplexer.rs`, `src/websocket/channel/types.rs:1448 LoC`.
  - **Why deferred:** Phase-2 once HTTP pipeline (item 1) is live.

- [ ] Response caching middleware with TTL per route.
  - **Files:** `src/middleware/caching.rs`.

- [ ] Circuit-breaker integration with load-balancer failover (`half-open` state).
  - **Files:** `src/loadbalancer/advanced.rs` (CircuitBreaker struct already at ~line 1100+).

- [ ] IP allowlist/blocklist filtering middleware (CIDR matching via `ipnet`).
  - **Files:** New `src/middleware/ip_filter.rs`.

## Low Priority / Future (one-liners)
- [ ] OpenAPI / Swagger auto-generation from router config.
- [ ] gRPC-Web ↔ gRPC proxying.
- [ ] mTLS to backend services (client-cert per upstream).
- [ ] Request deduplication for identical concurrent reads.
- [ ] WASM plugin system for user-supplied middleware.
- [ ] Traffic shadowing (mirror requests to secondary backend).
- [ ] A/B testing with weighted traffic splitting.
- [ ] Persistent rate-limit storage backend (Redis already feature-gated).
- [ ] HTTP/3 (QUIC) listener.
- [ ] Distributed-tracing span injection (W3C `traceparent` already in `versioning::negotiation`).

## Cross-crate dependencies
- **Blocks:** `oxigeo-server`, `oxigeo-services` (consumed as fronting gateway).
- **Blocked by:** None.

## Recently completed (verbatim)
*No prior `[x]` entries — slate was empty.*

---
*Last audited: 2026-05-16*

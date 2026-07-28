# TODO: oxigeo-gateway

> **Purpose:** Enterprise API gateway — rate-limiting, JWT/OAuth2 auth, GraphQL, WebSocket multiplexing, load balancing — for OxiGeo services.
> **Status (2026-07-28):** 17,917 LoC (src) · 381 tests (all-features) · 0 real-code stubs remaining from the 2026-05-16 audit (see Honest Limitations in README for what's intentionally deferred)
> **Roadmap:** v0.1.7 → v0.2.1 (current) → v1.0.0

## High Priority (verified gaps)
- [x] Implement the actual HTTP request pipeline in `handle_connection` (currently no-op).
  - **Done:** The old raw-`TcpListener` `Gateway::serve`/`handle_connection` no-op path is gone. `src/server/` now hosts a real axum `GatewayServer`/`GatewayServerBuilder` (`server/mod.rs`) assembled from `GatewayConfig` plus optional backends/handlers/components, with the effective layer order tracing → version negotiation → in-house middleware chain → authentication → rate limiting → request timeout → body-size limit → routes/fallback (see README "Route Table" / "Builder Options"). `Gateway::new(config)?.serve(addr)` now delegates to the same `GatewayServer`. Auth (JWT/API-key/session/RBAC/MFA), rate limiting, GraphQL, WebSocket, CORS/compression/caching middleware, and the load-balanced reverse proxy are all wired into this real router — the 381 tests exercise it end-to-end (including in-process `tower::oneshot` requests), not just as standalone data structures.
  - **Files:** `src/server/{mod,router,auth_layer,rate_limit_layer,versioning_layer,middleware_bridge,proxy,graphql,ws,state,error_response}.rs`.

- [x] Implement real HTTP backend probing in `LoadBalancer::custom_check` (currently logs and returns healthy).
  - **Done:** `http_check` (`src/loadbalancer/advanced.rs`) performs a genuine probe via `super::probe::http_probe` (honors `HealthCheckType::Https` upgrade, `expected_status_codes`, `expected_body`, custom headers, redirects) — this was already real, not the gap. The actual gap — `custom_check`/`grpc_check` silently returning `(true, None, None)` — is fixed: `custom_check` now delegates to a caller-registered `Arc<dyn CustomProbe>` (via `AdvancedHealthChecker::with_custom_probe`) and **fails closed** with an explanatory message when no probe is registered, instead of fabricating a healthy result. `grpc_check` documents that a full Pure-Rust `grpc.health.v1.Health/Check` (HTTP/2 + protobuf) probe isn't implemented and **fails closed** with a clear message rather than lying — consistent with this crate's "Honest Limitations" pattern. A native gRPC health-check protocol implementation remains open if ever needed.
  - **Files:** `src/loadbalancer/advanced.rs` (`CustomProbe` trait, `custom_check`, `grpc_check`, `http_check`).

- [ ] JWT validation via JWKS endpoint discovery with background key rotation.
  - **Goal:** `jsonwebtoken::DecodingKey` populated from a remote JWKS document (RFC 7517 §5) on startup and refreshed every `kid_rotation_interval`; `kid` header drives key selection; expired keys evicted.
  - **Design:** `JwksClient { url, cache: DashMap<kid, DecodingKey>, ttl }`. Background tokio task polls `${issuer}/.well-known/jwks.json` (RFC 8414 metadata discovery optional). Verification uses `Validation::new(alg).set_issuer(...).set_audience(...)` per RFC 7519 §4.1.
  - **Files:** `src/auth/jwt.rs` (already 299 LoC of HS256 + RS256 plumbing; add JWKS module).
  - **Tests:** (proposed) `test_jwks_fetch_and_decode`, `test_jwks_rotation_invalidates_stale_kid`, `test_jwks_missing_kid_returns_401`.
  - **Risk:** Time-of-check vs time-of-use — always re-check exp on every request.
  - **Prerequisites:** Request pipeline (item 1).

## Medium Priority
- [x] OAuth2 Authorization Code Flow with PKCE (RFC 7636).
  - **Done:** `src/auth/oauth2.rs` generates a fresh `code_verifier`/`code_challenge` (S256) pair per `get_authorization_url` call via `PkceCodeChallenge::new_random_sha256`, remembers the verifier keyed by `state` in `pkce_verifiers: Arc<DashMap<String, String>>`, includes `code_challenge`/`code_challenge_method=S256` in the authorization URL, and attaches the remembered verifier on token exchange. Client-credentials and password grants remain available alongside it.
  - **Files:** `src/auth/oauth2.rs`.

- [ ] API key rotation and revocation persistence (currently in-memory `DashMap`).
  - **Verified:** `src/auth/api_key.rs` still stores keys in a plain `Arc<DashMap<String, ApiKeyInfo>>` with no pluggable store trait found (the previous "pluggable store trait exists" note does not match current source) and no Redis/Postgres implementation.
  - **Files:** `src/auth/api_key.rs`.
  - **Why deferred:** Concrete persistent-store impl is downstream work.

- [ ] GraphQL query depth limiting + cost analysis (currently `async-graphql` accepts unbounded queries).
  - **Verified:** `GraphQLConfig` (`src/graphql/split/types.rs`) declares `max_depth`/`max_complexity` fields and a standalone `DepthCalculator` exists (`src/graphql/schema.rs`), but neither is invoked against incoming queries anywhere in `src/graphql/` or `src/server/graphql.rs` — the config fields are currently unused.
  - **Files:** `src/graphql/mod.rs`, `src/graphql/schema.rs`, `src/server/graphql.rs`.
  - **Why deferred:** Schema is intentionally small; wiring is the remaining step.

- [ ] WebSocket upgrade and backend forwarding (multiplexer exists; tunnel doesn't yet).
  - **Verified:** Still true — matches README's own "Honest Limitations": `/ws` terminates at the gateway's own `WebSocketManager`; no forward/tunnel/proxy code exists for upstream WebSocket connections.
  - **Files:** `src/websocket/multiplexer.rs`, `src/websocket/channel/types.rs`.
  - **Why deferred:** Phase-2 now that the real HTTP serving layer (item 1) is live.

- [ ] Response caching middleware with TTL per route.
  - **Verified:** `CachingMiddleware` (`src/middleware/caching.rs`) is real and wired into the serving layer's cache short-circuit (`server/middleware_bridge.rs`) with LRU + TTL eviction, but the TTL is a single global `CacheConfig.ttl`, not configurable per route.
  - **Files:** `src/middleware/caching.rs`.

- [x] Circuit-breaker integration with load-balancer failover (`half-open` state).
  - **Done:** `CircuitBreaker` (`src/loadbalancer/mod.rs`, `Closed`/`Open`/`HalfOpen` states) and the more advanced `EnhancedCircuitBreaker` (`src/loadbalancer/advanced.rs`, `permitted_number_of_calls_in_half_open`, `automatic_transition_from_open_to_half_open`) are wired into the real serving layer's `FailoverManager` (`src/server/state.rs`, `src/server/proxy.rs`): `retry_attempts` from config drives `FailoverConfig::max_retries`, and `proxy.rs` re-picks a backend each retry (open circuits filtered out by `select_backend`).
  - **Files:** `src/loadbalancer/{mod,advanced}.rs`, `src/server/{state,proxy}.rs`.

- [x] IP allowlist/blocklist filtering middleware (CIDR matching via `ipnet`).
  - **Done:** Implemented as an RBAC IP-based policy rather than a standalone `middleware/ip_filter.rs` — `src/auth/rbac.rs` has a hand-rolled `IpNetwork` CIDR-prefix parser (not the `ipnet` crate) supporting explicit CIDR notation and partial-octet prefixes, with blocked/allowed-prefix evaluation that fails closed on an unparseable client IP. Wired into the real request path via `src/server/auth_layer.rs` and `src/server/state.rs`.
  - **Files:** `src/auth/rbac.rs`, `src/server/auth_layer.rs`.

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
*No prior `[x]` entries as of 2026-05-16 — this audit (2026-07-28) is the first pass to flip items, covering the real axum serving-layer landing (`src/server/`), OAuth2 PKCE, load-balancer circuit breaker wiring, and the RBAC IP allow/blocklist policy.*

---
*Last audited: 2026-07-28*

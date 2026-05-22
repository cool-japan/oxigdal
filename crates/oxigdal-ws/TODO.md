# TODO: oxigdal-ws

> **Purpose:** WebSocket streaming (axum-based) for OxiGDAL — real-time tile / feature / event delivery with subscriptions, filtering, and backpressure.
> **Status (2026-05-16):** 3,866 LoC · 100 tests · 3 real-code stubs
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Replace zero-filled tile data with real tile generation in `TileHandler::generate_viewport_tiles`
  - **Verified gap:** `src/handlers/tiles.rs:119-124` — literal:
    `// Generate tiles (placeholder - would integrate with actual tile generation)  let mut tile_data = Vec::new();  for (x, y) in tiles {  // This would call actual tile generation from OxiGDAL  let data = vec![0u8; 256]; // Placeholder`
  - **Goal:** Streamed tiles carry real pixel data. Handler accepts a pluggable `TileProvider` trait so callers wire in `oxigdal-pmtiles`, `oxigdal-mbtiles`, or `oxigdal-services` tile sources without modifying this crate.
  - **Design:** Introduce `pub trait TileProvider: Send + Sync + 'static { async fn fetch(&self, z: u8, x: u32, y: u32, fmt: TileFormat) -> Result<TileData>; }`. `TileHandler::new(provider: Arc<dyn TileProvider>)`. `generate_viewport_tiles` calls `provider.fetch(zoom, x, y, format)` per coord. For backwards-compatible callers, ship an `EmptyTileProvider` that returns the existing 256-byte zero pattern with an explicit deprecation note. `oxigdal-services` can plug its own `PmTilesProvider` / `XyzProvider`.
  - **Files:** `src/handlers/tiles.rs:90-150`; new `src/handlers/tile_provider.rs`.
  - **Tests:** (proposed) `test_tile_handler_invokes_provider_per_tile`, `test_tile_handler_propagates_provider_errors_as_close_frame`, `test_tile_handler_with_empty_provider_returns_legacy_pattern`.
  - **Risk:** `TileProvider` boxing adds an async trait object — `async-trait` already in workspace; no MSRV issue.
  - **Prerequisites:** None.

- [ ] Implement attribute-based feature filtering inside `FeatureHandler::stream_features`
  - **Verified gap:** `src/handlers/features.rs:260` — literal: `// For now, include all` (inside the per-feature loop where `SubscriptionFilter::Attribute` predicates should be evaluated)
  - **Goal:** Subscribers receive only features whose attributes match the `SubscriptionFilter::Attribute { key, op, value }` predicate (already declared in `src/protocol.rs::SubscriptionFilter` — see `src/lib.rs:84` re-exporting `SubscriptionFilter`).
  - **Design:** Build a small evaluator `AttributeMatcher::matches(props: &serde_json::Map<String, Value>) -> bool` supporting ops `Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge`, `In`, `Like`. Run inside the feature-streaming loop before the WebSocket send call. Track skipped count in `FeatureStreamStats`.
  - **Files:** `src/handlers/features.rs:255-275` (the filter loop); new `src/handlers/attribute_matcher.rs`.
  - **Tests:** (proposed) `test_attribute_filter_eq_drops_mismatch`, `test_attribute_filter_lt_passes_lower_values`, `test_attribute_filter_in_set_membership`, `test_attribute_filter_like_glob_pattern`, `test_attribute_filter_missing_key_evaluates_as_false`.
  - **Risk:** Numeric coercion (JSON `1.0` vs `1`) — document explicit-type semantics in `SubscriptionFilter` docstring.
  - **Prerequisites:** None.

- [ ] Wire prefetch path in `TileHandler` to the same provider (currently same placeholder pattern repeated)
  - **Verified gap:** `src/handlers/tiles.rs:179` — literal: `// Generate tiles for prefetch area` (followed by the same zero-byte fill path as the viewport handler above)
  - **Goal:** Tile prefetch fetches real bytes from `TileProvider` and primes the client-side LRU.
  - **Design:** Reuse the `TileProvider` trait from item 1 above; `prefetch_tiles` simply iterates over the prefetch coord set with `provider.fetch(...)`. Optional `prefetch_concurrency` cap via `tokio::sync::Semaphore` to avoid overloading the provider.
  - **Files:** `src/handlers/tiles.rs:175-210`.
  - **Tests:** (proposed) `test_prefetch_uses_provider_not_placeholder`, `test_prefetch_respects_concurrency_limit`, `test_prefetch_short_circuits_on_provider_error`.
  - **Risk:** Same as item 1.
  - **Prerequisites:** Item 1 (`TileProvider` trait).

## Medium Priority
- [ ] TLS support (`wss://`) via `axum-server::tls_rustls`
  - **Goal:** Browser clients on HTTPS pages can only connect to `wss://`. Add config.
  - **Files:** `src/server.rs:160-167` (the `tokio::net::TcpListener::bind` + `axum::serve` path).
  - **Why deferred:** Workspace already uses `rustls`; needs a wired `ServerConfig::tls: Option<TlsConfig>`.

- [ ] MessagePack binary protocol path symmetric with JSON
  - **Goal:** `rmp-serde` already in deps; bind a `MessageFormat::MessagePack` codec parallel to JSON.
  - **Files:** `src/protocol.rs` (26.8 KB).
  - **Why deferred:** Protocol enum exists; codec dispatch missing.

- [ ] Subscription manager with combined spatial/temporal/attribute filters
  - **Goal:** Compose `SubscriptionFilter` variants per the protocol; current code applies one filter at a time.
  - **Files:** `src/subscription.rs` (12.6 KB).
  - **Why deferred:** Needs attribute-matcher (high-priority item 2) first.

- [ ] Backpressure controller with adaptive flow control
  - **Goal:** Honour client `flow_control` hints; throttle when client buffer fills.
  - **Files:** `src/stream.rs` (10.9 KB exports `BackpressureController`, `BackpressureState`).
  - **Why deferred:** Strategy choice (drop vs throttle) needs design vote.

- [ ] Delta encoder for tile updates (xor-based diff between versions)
  - **Goal:** Reduce bandwidth for slowly-evolving tiles.
  - **Files:** `src/stream.rs::DeltaEncoder`.
  - **Why deferred:** Requires server-side version tracking.

- [ ] Client reconnection with message replay from sequence number
  - **Goal:** `client.rs` does not buffer last-seen seq; on reconnect server cannot replay missed messages.
  - **Files:** `src/client.rs` (14.3 KB), `src/server.rs`.
  - **Why deferred:** Requires server-side ring buffer.

- [ ] Health-check HTTP endpoint alongside the WebSocket upgrade route
  - **Goal:** Liveness/readiness probes without WS handshake.
  - **Files:** `src/server.rs` (already uses axum `Router`).
  - **Why deferred:** Trivial; add when first deployed under k8s.

## Low Priority / Future (one-liners)
- [ ] Load-testing harness simulating thousands of concurrent clients
- [ ] Multi-node broadcast bus (Redis pub/sub or NATS backend)
- [ ] AsyncAPI spec auto-generation from the `Message` enum
- [ ] Python client SDK via PyO3 bindings
- [ ] gRPC-Web → WebSocket gateway for protocol translation
- [ ] Observability — trace-context propagation in WS frames

## Cross-crate dependencies
- **Blocks:** oxigdal-services (real-time push)
- **Blocked by:** None
- **Sibling:** oxigdal-websocket (raw tokio-tungstenite; this crate is axum-based — keep both, do not duplicate functionality between them)

## Recently completed (verbatim)
- *(none in this slice)*

---
*Last audited: 2026-05-16*

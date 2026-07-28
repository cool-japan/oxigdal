# TODO: oxigeo-ws

> **Purpose:** WebSocket streaming (axum-based) for OxiGeo — real-time tile / feature / event delivery with subscriptions, filtering, and backpressure.
> **Status (2026-07-28):** 4,590 LoC · 63 tests · 1 real-code stub remains (attribute-based feature filtering) — the `TileHandler` viewport/prefetch zero-fill stubs from the prior audit are now implemented via a pluggable `TileGenerator`.
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Replace zero-filled tile data with real tile generation in `TileHandler::generate_viewport_tiles`
  - **Done:** 2026-07-21 (0.2.1 production campaign). Implemented via a pluggable `TileGenerator` closure type rather than a boxed async trait: `pub type TileGenerator = Arc<dyn Fn(u32, u32, u8) -> Result<TileData> + Send + Sync>`. `TileHandler` gained an `Option<TileGenerator>` field wired through `with_tile_source` (builder-style) / `set_tile_source` / `has_tile_source`. `generate_viewport_tiles` now returns `Error::NotFound` when no source is configured — instead of fabricating placeholder bytes — and otherwise calls the generator per tile coordinate, logging and skipping (not aborting) individual tile failures.
  - **Original gap (resolved):** `src/handlers/tiles.rs:119-124` used to read `// Generate tiles (placeholder - would integrate with actual tile generation)` / `let data = vec![0u8; 256]; // Placeholder`.

- [ ] Implement attribute-based feature filtering inside `FeatureHandler::stream_features`
  - **Verified gap:** `src/handlers/features.rs:260` — literal: `// For now, include all` (inside the per-feature loop where `SubscriptionFilter::Attribute` predicates should be evaluated)
  - **Goal:** Subscribers receive only features whose attributes match the `SubscriptionFilter::Attribute { key, op, value }` predicate (already declared in `src/protocol.rs::SubscriptionFilter` — see `src/lib.rs:84` re-exporting `SubscriptionFilter`).
  - **Design:** Build a small evaluator `AttributeMatcher::matches(props: &serde_json::Map<String, Value>) -> bool` supporting ops `Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge`, `In`, `Like`. Run inside the feature-streaming loop before the WebSocket send call. Track skipped count in `FeatureStreamStats`.
  - **Files:** `src/handlers/features.rs:255-275` (the filter loop); new `src/handlers/attribute_matcher.rs`.
  - **Tests:** (proposed) `test_attribute_filter_eq_drops_mismatch`, `test_attribute_filter_lt_passes_lower_values`, `test_attribute_filter_in_set_membership`, `test_attribute_filter_like_glob_pattern`, `test_attribute_filter_missing_key_evaluates_as_false`.
  - **Risk:** Numeric coercion (JSON `1.0` vs `1`) — document explicit-type semantics in `SubscriptionFilter` docstring.
  - **Prerequisites:** None.

- [x] Wire prefetch path in `TileHandler` to the same provider (currently same placeholder pattern repeated)
  - **Done:** 2026-07-21 (0.2.1 production campaign). `prefetch_tiles` computes the expanded prefetch bbox (`expand_bbox`) and delegates straight to the now-real `generate_viewport_tiles` (item above), so it shares the same `TileGenerator` and the same `Error::NotFound`-when-unconfigured behavior — no separate placeholder path remains. No `prefetch_concurrency` semaphore was added (calls are sequential per-tile via the shared generator); left as a possible future optimization, not a correctness gap.
  - **Original gap (resolved):** `src/handlers/tiles.rs:179` used to read `// Generate tiles for prefetch area` followed by the same zero-byte fill path as the viewport handler.

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
- **Blocks:** oxigeo-services (real-time push)
- **Blocked by:** None
- **Sibling:** oxigeo-websocket (raw tokio-tungstenite; this crate is axum-based — keep both, do not duplicate functionality between them)

## Recently completed (verbatim)
- [x] Real tile generation via pluggable `TileGenerator` for both viewport (`generate_viewport_tiles`) and prefetch (`prefetch_tiles`) — `src/handlers/tiles.rs`

---
*Last audited: 2026-07-28*

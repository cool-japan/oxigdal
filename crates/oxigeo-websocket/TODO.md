# TODO: oxigeo-websocket

> **Purpose:** Real-time WebSocket communication (raw `tokio-tungstenite`) for OxiGeo — broadcasting, pub/sub, live tile/feature updates.
> **Status (2026-07-28):** 7,533 LoC · 128 tests · 1 real-code stub
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Wire `GeoBboxFilter::should_deliver` to actually consult message coordinates (currently accepts all)
  - **Verified gap:** `src/broadcast/filter.rs:145-150` — literal:
    `impl FilterPredicate for GeoBboxFilter {  fn should_deliver(&self, _message: &Message, _connection_id: &ConnectionId) -> bool {  // In a real implementation, would extract coordinates from message  // For now, accept all  true  }  }`
  - **Goal:** Bounded-box subscriptions filter messages whose embedded coordinates fall outside the registered bbox. A subscriber to `bbox=[lon0, lat0, lon1, lat1]` must NOT receive a feature with `lon < lon0`.
  - **Design:** Extend `Payload` (see `src/protocol/message.rs` — `Message { id, msg_type, timestamp, payload, correlation_id }`) with an optional `coord: Option<(f64, f64)>` extractor or a typed `Payload::Feature { geometry: oxigeo_geo::Geometry, ... }` variant. `should_deliver` then runs `self.contains(coord.0, coord.1)` from the existing `GeoBboxFilter::contains(&self, x: f64, y: f64) -> bool` at `src/broadcast/filter.rs:140-142`. For non-spatial payloads, default to "deliver" (preserve current behaviour for non-feature traffic).
  - **Files:** `src/broadcast/filter.rs:145-150` (impl body); `src/protocol/message.rs` (Payload variants); `src/protocol/binary.rs` / `json.rs` codecs (extract coords during decode).
  - **Tests:** (proposed) `test_geobbox_filter_drops_messages_outside_bbox`, `test_geobbox_filter_passes_messages_inside_bbox`, `test_geobbox_filter_passes_non_spatial_messages_unchanged`, `test_geobbox_filter_handles_dateline_wrap_around_180`.
  - **Risk:** Dateline crossing (bbox `[170, -10, -170, 10]`) — document expected wrap-around behaviour.
  - **Prerequisites:** None.

- [x] Add per-connection authentication on the WebSocket upgrade (bearer-token validation)
  - **Done:** 2026-07-21 (0.2.1 production campaign). Implemented as a static bearer-token/role registry rather than JWT (simpler, no external key-management surface): `handle_connection` (`src/server/ws_server.rs`) now calls `accept_hdr_async(stream, callback)`; the callback extracts a token from `Authorization: Bearer <token>` (`token_from_authorization`) or a `?token=` query parameter (`token_from_query`), calls `AuthConfig::authenticate` (`src/server/auth.rs`), and returns HTTP 401 with a reason body before the upgrade completes on missing/invalid token. Open mode (`AuthConfig::open()`, the default) grants every handshake an anonymous `Role::Admin` principal, preserving prior no-auth behavior; `AuthConfig::with_token` / `add_token` register accepted tokens once auth is enabled, and `AuthPrincipal::authorize(required_role)` gates role-sensitive operations. Tests in `src/server/auth.rs`: `test_open_mode_grants_admin`, `test_token_validation`, `test_authorize`, `test_token_extractors`.
  - **Original gap (resolved):** `src/server/ws_server.rs:118-142` used to call `accept_async(stream)` immediately after `listener.accept().await` with no header inspection.

## Medium Priority
- [ ] Heartbeat with stale-connection cleanup (ping/pong + idle timeout)
  - **Goal:** `src/server/heartbeat.rs` (6 KB) defines the protocol but the server loop at `src/server/ws_server.rs:142+` does not drive ping every `HeartbeatConfig::interval`. Wire it.
  - **Files:** `src/server/ws_server.rs`, `src/server/heartbeat.rs`.
  - **Why deferred:** Tests cover the timer; needs integration into the per-connection task loop.

- [ ] permessage-deflate compression extension (RFC 7692)
  - **Goal:** Negotiate `permessage-deflate` during handshake; reduce bandwidth for JSON tile updates.
  - **Files:** `src/protocol/compression.rs` (7.4 KB scaffold), `src/server/ws_server.rs`.
  - **Why deferred:** `tokio-tungstenite` 0.29 does not support permessage-deflate natively; needs `tungstenite-deflate` shim.

- [ ] Per-connection backpressure with configurable buffer high-water mark
  - **Goal:** Drop or coalesce updates when a slow client falls behind, instead of unbounded growth in the per-connection mpsc.
  - **Files:** `src/server/connection.rs` (10 KB), `src/server/manager.rs` (8.6 KB).
  - **Why deferred:** Needs design decision: drop-oldest vs. drop-newest vs. coalesce.

- [ ] Rate limiting per connection + per IP
  - **Goal:** Token-bucket guard against abusive clients.
  - **Files:** new `src/server/rate_limit.rs`.
  - **Why deferred:** External dep (`governor`); confirm workspace policy.

- [ ] Tile delta encoding (send only changed pixels between subsequent versions of same tile)
  - **Goal:** XOR-diff payloads for raster updates; significant savings for slow-changing tiles.
  - **Files:** `src/updates/` (current scaffold has `TileUpdateManager`).
  - **Why deferred:** Requires version tracking on the server side.

- [ ] Graceful shutdown with drain timeout
  - **Goal:** On `Server::shutdown()`, stop accepting new connections, send Close frames to existing, await drain or timeout.
  - **Files:** `src/server/ws_server.rs`.
  - **Why deferred:** Needs cancellation-token plumbing through `ServerInner`.

## Low Priority / Future (one-liners)
- [ ] Server-Sent Events (SSE) fallback path for non-WebSocket environments
- [ ] WebTransport (HTTP/3) support when `quinn` ecosystem matures
- [ ] Multi-cluster WebSocket federation (cross-node broadcast bus)
- [ ] Message replay (catch-up on reconnect via sequence number)
- [ ] Proxy-protocol parsing for `X-Forwarded-For` / haproxy mode
- [ ] AsyncAPI 2.x spec auto-generation from `Message` enum

## Cross-crate dependencies
- **Blocks:** oxigeo-services (real-time push)
- **Blocked by:** None
- **Sibling:** oxigeo-ws (axum-based; this crate is raw tokio-tungstenite — keep both, do not duplicate functionality between them)

## Recently completed (verbatim)
- [x] Per-connection bearer-token authentication on the WebSocket upgrade — `src/server/ws_server.rs`, `src/server/auth.rs`

---
*Last audited: 2026-07-28*

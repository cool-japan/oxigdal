# TODO: oxigeo-edge

> **Purpose:** Edge-computing runtime — offline-first cache, edge-to-cloud sync (CRDT conflict resolution), local resource monitoring, adaptive compression for bandwidth-limited links.
> **Status (2026-05-17):** 4,067 LoC · 75 #[test]/#[tokio::test] attributes · 2 real-code soft stubs (plus the sync layer is currently `MockSyncProtocol`-only).
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Replace `MockSyncProtocol` with a real HTTP/2 transport
  - **Verified gap:** `src/sync/manager.rs:3` — `use super::protocol::{MockSyncProtocol, SyncProtocol};`; `src/sync/manager.rs:27` — `let protocol: Arc<dyn SyncProtocol> = Arc::new(MockSyncProtocol::new());`. The `SyncManager` constructor *always* wires the in-process mock — never an HTTP/gRPC client. `src/sync/protocol.rs:86` defines `MockSyncProtocol`; `src/sync/protocol.rs:166` carries `// Pull remote items (simplified)`.
  - **Goal:** A real `HttpSyncProtocol` implementing `SyncProtocol` over reqwest/`hyper` with HTTP/2 multiplexing, server-side resumable uploads, and ETag-based idempotency keys. `SyncManager::new(config)` selects the protocol based on `config.endpoint`: `Some(url)` → HTTP, `None` → in-process mock for tests.
  - **Design:** Add `reqwest = { workspace = true, default-features = false, features = ["http2", "rustls-tls", "stream"] }` (Pure Rust TLS via rustls). Wire endpoints:
    - `POST /v1/sync/push` with `Content-Type: application/octet-stream` body of zstd-compressed `Vec<SyncItem>` (use `oxiarc-zstd` from workspace).
    - `GET /v1/sync/pull?since=<rfc3339>` returning the same encoding.
    - `POST /v1/sync/exchange` for the symmetric sync.
    - Bearer-token auth via `Authorization: Bearer <token>` from `EdgeConfig::auth_token`.
    - Idempotency: `Idempotency-Key: <uuid>` header on push, server returns 200/409.
    - Resumable upload via `Range: bytes=<offset>-` on retry (server must support).
  - **Files:** (new) `crates/oxigeo-edge/src/sync/http_protocol.rs`; `crates/oxigeo-edge/src/sync/manager.rs` (constructor dispatch); `crates/oxigeo-edge/src/sync/mod.rs` (re-export); `crates/oxigeo-edge/Cargo.toml` (add reqwest, uuid).
  - **Tests:** (proposed) `test_http_sync_push_returns_200_on_success`, `test_http_sync_pull_returns_items_since_timestamp`, `test_http_sync_retries_on_503_with_backoff`, `test_http_sync_propagates_409_idempotent_conflict`, `test_http_sync_authorization_header_attached`, `test_sync_manager_with_endpoint_uses_http_protocol`, `test_sync_manager_without_endpoint_uses_mock_protocol`.
  - **Risk:** Adding reqwest grows the binary by ~600 KB; gate behind a `sync-http` feature so embedded-edge users can opt out. Pure-Rust TLS via rustls satisfies COOLJAPAN policy.
  - **Prerequisites:** None.

- [ ] Real CPU + memory + disk sampling (replace mock `0.0` in scheduler heartbeat)
  - **Verified gap:** `src/runtime/scheduler.rs:88-89` — `// Collect CPU sample (simplified - in real implementation would use sysinfo)`; `src/runtime/scheduler.rs:103-107` — `/// Sample CPU usage (simplified)` / `fn sample_cpu() -> f64 {` / `// In a real implementation, this would use platform-specific APIs` / `// For now, return a mock value` / `0.0`. Every heartbeat records 0% CPU into `ResourceMetrics`, so adaptive throttling logic downstream is blind.
  - **Goal:** Heartbeat samples real CPU% (rolling 1-second average), RSS memory bytes, free disk space on the cache dir, and active socket count. `ResourceManager` records these and `Scheduler` uses them to make admission-control decisions.
  - **Design:** Use `sysinfo` (already a workspace dep used by oxigeo-mobile-enhanced). Hold a `Arc<RwLock<sysinfo::System>>` in `Scheduler` (mirror battery-monitor pattern). `sample_cpu` calls `sys.refresh_cpu_specifics(CpuRefreshKind::everything())` then averages `sys.cpus().iter().map(|c| c.cpu_usage())`. For memory: `sys.process(get_current_pid())?.memory()` for RSS, or `sys.used_memory()` for system-wide. For disk: `statvfs(cache_dir)` on Unix, `GetDiskFreeSpaceEx` on Windows (gate cfg). All sampling has a 200ms upper bound to keep heartbeat snappy.
  - **Files:** `crates/oxigeo-edge/src/runtime/scheduler.rs` (replace `sample_cpu`, add memory/disk samplers); `crates/oxigeo-edge/src/resource.rs` (extend `ResourceMetrics`); `crates/oxigeo-edge/Cargo.toml` (add `sysinfo` direct dep).
  - **Tests:** (proposed) `test_sample_cpu_returns_value_between_0_and_100`, `test_sample_memory_returns_positive_rss`, `test_sample_disk_free_returns_realistic_value`, `test_scheduler_heartbeat_records_three_metric_kinds`, `test_resource_manager_metrics_after_heartbeat_nonzero`.
  - **Risk:** sysinfo on macOS calls some private APIs that may trigger TCC prompts in sandboxed contexts — document. For containers, `/proc/self/status` parsing is the standard fallback.
  - **Prerequisites:** None.

- [ ] Persistent SQLite (or sled) cache storage backend
  - **Verified gap:** Existing TODO line — `[ ] Add persistent cache storage backend (SQLite or file-based)`. `src/cache.rs` (15.3K) wraps an in-process `lru::LruCache`; the `CacheConfig::persistent: bool` + `cache_dir: Option<PathBuf>` fields exist but are not honored (verified — no file I/O in cache.rs).
  - **Goal:** When `CacheConfig { persistent: true, cache_dir: Some("/data/cache") }`, the cache survives process restarts. Items spill from in-memory LRU to disk; cold-cache reads pull from disk; eviction respects both memory and disk budgets.
  - **Design:** Use `sled` (already in workspace, already in this crate's `Cargo.toml`). Schema: one tree per `CacheKey` namespace; value is a `bincode` (use **oxicode** per COOLJAPAN policy — never raw bincode) serialized `CacheEntry`. On `get()`, check in-memory LRU first; on miss, check sled; promote to LRU. On `put()`, write through to sled if persistent. Eviction: drop oldest sled entries when `du(sled_tree) > max_size_bytes`. Compress entries with `oxiarc-zstd` (already a dep) when `data.len() > 4 KB`.
  - **Files:** `crates/oxigeo-edge/src/cache.rs` (sled backend); (new) `crates/oxigeo-edge/src/cache/persistent.rs`; `crates/oxigeo-edge/Cargo.toml` (`oxicode` for serialization — replacing any bincode usage if found).
  - **Tests:** (proposed) `test_persistent_cache_roundtrip_survives_restart`, `test_persistent_cache_lru_promotes_on_disk_hit`, `test_persistent_cache_evicts_oldest_when_disk_quota_exceeded`, `test_persistent_cache_zstd_compresses_above_threshold`, `test_persistent_cache_concurrent_writes_safe`.
  - **Risk:** sled has known unmaintained deps (RUSTSEC-2025-0057 fxhash, RUSTSEC-2024-0384 instant) per workspace audit — already accepted in MEMORY.md's allowlist. Consider migrating to `redb` for v0.2.0 (no transitive RUSTSEC items).
  - **Prerequisites:** None.

- [ ] Real CRDT merge for concurrent edit conflict resolution
  - **Verified gap:** Existing TODO line — `[ ] Implement real conflict resolution with CRDT merge for concurrent edits`. `src/conflict.rs` (14.7K) declares `CrdtMap`, `CrdtSet`, `VectorClock` — verify operational-vs-state CRDT semantics are correct under concurrent edits.
  - **Goal:** OR-Set (Observed-Remove Set) for set values, LWW-Element-Set (Last-Writer-Wins) for map values keyed by vector clock, RGA (Replicated Growable Array) for sequence types. Merge is commutative, associative, idempotent — verified by property-based tests with `proptest` (already a dev-dep).
  - **Design:** Reference: Shapiro et al. "Conflict-free Replicated Data Types" (INRIA 2011). Implement:
    - **OR-Set**: each add carries a unique tag (UUID); remove only removes observed tags; merge unions tagged-adds, intersects tagged-removes. Concurrent add+remove → add wins.
    - **LWW-Map**: each value carries a Lamport timestamp + node ID; merge keeps the latest timestamp, breaks ties on node ID.
    - **VectorClock**: dotted-version-vector for causality tracking; `happens_before(a, b)` strict partial order.
    - **Merge protocol**: at sync time, exchange compact deltas (not full state) by sending operations since the peer's last seen vector clock.
  - **Files:** `crates/oxigeo-edge/src/conflict.rs` (validate / extend); (new) `crates/oxigeo-edge/src/conflict/or_set.rs`, `crates/oxigeo-edge/src/conflict/lww_map.rs`, `crates/oxigeo-edge/src/conflict/rga.rs`.
  - **Tests:** (proposed via proptest) `proptest_or_set_add_remove_commutative`, `proptest_lww_map_merge_idempotent`, `proptest_vector_clock_strict_partial_order`, `test_rga_concurrent_inserts_at_same_position_deterministic`, `test_crdt_merge_three_way_associative`.
  - **Risk:** RGA is complex; consider deferring sequence CRDT to v0.2.0.
  - **Prerequisites:** None.

- [ ] Edge node discovery + mesh networking (mDNS + UDP gossip)
  - **Verified gap:** Existing TODO line — `[ ] Implement edge node discovery and mesh networking between nearby nodes`. No discovery code in `src/` today (verified).
  - **Goal:** Edge nodes on the same LAN auto-discover each other via mDNS (`_oxigeo-edge._tcp.local`), exchange capability vectors, and form a gossip mesh for peer-to-peer cache sync that bypasses cloud round-trips.
  - **Design:** Use `mdns-sd 0.13` (Pure Rust, no_std-compatible, in active maintenance) for service discovery. Each node advertises `{ node_id: Uuid, version: semver, capabilities: bitmask, port: u16, last_sync: u64 }` under TXT records. For gossip: use `swim` protocol (SWIM: Scalable Weakly-consistent Infection-style Process Group Membership; Das, Gupta, Motivala, 2002) — periodic UDP heartbeats + indirect-ping fallback. Cap mesh size at 32 nodes per locality; partition large meshes via consistent hashing.
  - **Files:** (new) `crates/oxigeo-edge/src/mesh/discovery.rs`, `crates/oxigeo-edge/src/mesh/gossip.rs`, `crates/oxigeo-edge/src/mesh/membership.rs`; modify `crates/oxigeo-edge/Cargo.toml` to add `mdns-sd`.
  - **Tests:** (proposed) `test_mdns_advertise_discoverable_on_same_host`, `test_gossip_membership_converges_under_3_nodes_simulation`, `test_swim_indirect_ping_detects_partition`, `test_mesh_capability_negotiation`.
  - **Risk:** mDNS doesn't traverse most NAT — document that mesh is LAN-only; cross-LAN nodes still go via cloud. mdns-sd has UDP socket buffer requirements that may need OS tuning.
  - **Prerequisites:** Item 1 (HTTP transport for cross-LAN fallback).

## Medium Priority
- [ ] Delta compression for sync payloads (rsync-style rolling hash)
  - **Goal:** When pushing a modified raster, only the changed tiles travel.
  - **Files:** `crates/oxigeo-edge/src/compression.rs` (extend); (new) `crates/oxigeo-edge/src/compression/rdiff.rs`.
  - **Why deferred:** Pending Item 1 (real HTTP transport).

- [ ] Priority-based sync queue (critical items first, deferred uploads last)
  - **Goal:** `SyncManager::enqueue(item, Priority::Critical)`; multi-level feedback queue.
  - **Files:** `crates/oxigeo-edge/src/sync/queue.rs` (new).
  - **Why deferred:** Pending Item 1.

- [ ] Edge-side ML inference scheduling with model versioning
  - **Goal:** Run small ONNX-Runtime models locally; rotate model versions.
  - **Files:** (new) `crates/oxigeo-edge/src/ml/inference.rs`.
  - **Why deferred:** Coordinated with oxigeo-ml.

- [ ] Data retention policy with configurable TTL
  - **Goal:** Items older than `ttl_secs` get evicted regardless of LRU.
  - **Files:** `crates/oxigeo-edge/src/cache.rs` (extend with TTL sweep).
  - **Why deferred:** Quick win once Item 3 (persistent cache) lands.

- [ ] Bandwidth-aware sync (defer big uploads to WiFi)
  - **Goal:** Don't push >1 MB items on metered cellular.
  - **Files:** `crates/oxigeo-edge/src/sync/manager.rs` (network check).
  - **Why deferred:** Requires network-type detection from oxigeo-mobile-enhanced.

- [ ] Write-ahead log for crash-safe local operations
  - **Goal:** `WriteAheadLog::append(op) -> sequence_id; commit(sequence_id)` for atomic batches.
  - **Files:** (new) `crates/oxigeo-edge/src/wal.rs`.
  - **Why deferred:** Pending Item 3 — sled already provides crash safety; evaluate before duplicating.

- [ ] Edge cluster coordination with Raft leader election
  - **Goal:** One node coordinates writes; followers replicate.
  - **Files:** (new) `crates/oxigeo-edge/src/cluster/raft.rs`.
  - **Why deferred:** Heavy — only justified for multi-node deployments.

- [ ] Adaptive compression algorithm selection by data type
  - **Goal:** zstd for tiles, lz4 for hot path, deflate for compatibility.
  - **Files:** `crates/oxigeo-edge/src/compression.rs` (extend `AdaptiveCompressor`).
  - **Why deferred:** Mostly already exists; verify dispatch logic.

## Low Priority / Future (one-liners)
- [ ] MQTT/AMQP broker integration for event-driven sync.
- [ ] Geographic sharding for multi-region edge deployments.
- [ ] Edge analytics with local aggregation pre-cloud-upload.
- [ ] Secure enclave (TPM / Apple Secure Enclave) credential storage.
- [ ] Container / WASI runtime for edge function deployment.
- [ ] Predictive prefetch based on access patterns + time-of-day.
- [ ] Edge-to-edge direct relay (works during cloud outage).
- [ ] Migrate sled → redb to retire RUSTSEC-2025-0057 / RUSTSEC-2024-0384.

## Cross-crate dependencies
- **Blocks:** oxigeo-mobile-enhanced (sync infrastructure).
- **Blocked by:** oxigeo-ml (edge inference), oxigeo-mobile-enhanced (network-type detection for bandwidth-aware sync).

## Recently completed (verbatim)
- (no `[x]` entries in prior TODO.md — see README.md for the offline-first architecture; `.edge_minimal/` and `.oxigeo_cache/` directories indicate prior test runs)

---
*Last audited: 2026-05-17*

# TODO: oxigeo-edge

> **Purpose:** Edge-computing runtime — offline-first cache, edge-to-cloud sync (CRDT conflict resolution), local resource monitoring, adaptive compression for bandwidth-limited links.
> **Status (2026-07-28):** 3,535 LoC · 89 tests all-features / 86 default-features · real HTTP sync (`http-sync` feature), sysinfo CPU sampling, sled persistent cache, and CRDT merge all now implemented (see Recently completed); mesh/discovery remains unimplemented.
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Replace `MockSyncProtocol` with a real HTTP transport
  - **Verified done:** `src/sync/protocol.rs:272` defines a real `pub struct HttpSyncProtocol` with a full `impl SyncProtocol for HttpSyncProtocol` (`:301`), config type `HttpSyncConfig` (`:205`), and is exercised in `src/sync/protocol.rs:436+` against "a minimal hand-rolled HTTP/1.1" test server. `manager.rs:28` documents it as the real alternative to `MockSyncProtocol`. Gated behind a non-default `http-sync = ["sync", "dep:reqwest"]` Cargo feature (`Cargo.toml:79`) — intentionally excluded from `default`/`all` because reqwest's rustls backend pulls `aws-lc-sys` (C+asm), the same accepted trade-off documented elsewhere in the workspace (e.g. `oxigeo-etl`'s `kafka` feature).
  - **Delta from original design:** confirmed HTTP/1.1 request/response cycle in tests; HTTP/2 multiplexing, ETag idempotency keys, and `Range`-based resumable upload were not independently verified — re-check `protocol.rs` directly if those specific properties matter for a release claim.

- [x] Real CPU sampling (replace mock `0.0` in scheduler heartbeat) — PARTIAL, memory/disk still mocked
  - **Verified done:** `src/runtime/scheduler.rs:9` imports `sysinfo::{CpuRefreshKind, RefreshKind, System}`; `sample_cpu` now samples real host CPU utilization via `sysinfo` with a `sysinfo::MINIMUM_CPU_UPDATE_INTERVAL`-respecting two-sample pattern (tests sleep for that interval between samples, `:278,316`), replacing the old `fn sample_cpu() -> f64 { 0.0 }` stub referenced by its own removal comment at `:311`.
  - **Gap remaining:** `rg "fn sample_memory|fn sample_disk" src/runtime/scheduler.rs` returns no matches — RSS memory and free-disk-space sampling from the original goal were **not** added; only the CPU third of the three-metric goal is done. Leave this partially open until memory/disk sampling lands.

- [x] Persistent SQLite (or sled) cache storage backend
  - **Verified done:** `src/cache.rs:150` holds `persistent_storage: Option<sled::Db>`; when `config.persistent` is set, `sled::open(cache_dir)` is called (`:172-174`) and both `get`/`put`/remove paths check/write through to the sled tree (`:209-210, 271, 298-299, 322`). This matches the TODO's own suggested design (sled, already a workspace+crate dep) rather than a from-scratch SQLite backend.
  - **Not independently re-verified:** zstd-above-4KB compression on entries, and whether values are oxicode- vs some-other-encoding-serialized — re-check `cache.rs` directly if those specific sub-claims matter.

- [x] Real CRDT merge for concurrent edit conflict resolution — PARTIAL (different primitives than originally sketched)
  - **Verified done:** `src/conflict.rs` implements real, non-trivial CRDTs, each with its own `merge()`: `VectorClock::merge` (`:38`), `LwwRegister<T>::merge` (`:140`), `GSet<T>::merge` (`:201`, grow-only set), `TwoPhaseSet<T>::merge` (`:271`, 2P-Set), and `CrdtMap<K, V>::merge` (`:350`, backed by per-key `LwwRegister`). This is genuine conflict-resolution logic, not a pass-through.
  - **Delta from original design:** the TODO proposed OR-Set (tag-based, supports re-add after remove) + a Lamport-timestamp LWW-Map + RGA for sequences. What's actually implemented is `GSet`/`TwoPhaseSet` (2P-Set cannot re-add after remove — strictly weaker than OR-Set) + `LwwRegister`-per-key `CrdtMap` (no explicit node-ID tie-break documented) + no RGA/sequence CRDT at all. Commutativity/associativity/idempotence are not verified by `proptest` (no `proptest_` prefixed tests found in `conflict.rs`) — only example-based unit tests. If OR-Set re-add semantics or property-based merge guarantees are required, treat this as still open.

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
*Last audited: 2026-07-28 (status line refreshed: 75→89/86 tests, LoC 4,067→3,535, date bumped; all four High Priority items re-verified against source and flipped to done — CPU sampling and CRDT merge marked PARTIAL since memory/disk sampling and OR-Set/RGA/property-based-merge guarantees are still missing; mesh/discovery re-checked and confirmed still absent — left open)*

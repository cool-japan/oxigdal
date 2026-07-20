# TODO: oxigeo-sync

> **Purpose:** Multi-device synchronization with CRDTs, vector clocks, and operational transformation for OxiGeo (LWW-Register, G-Counter, PN-Counter, OR-Set; vector clocks; Merkle trees; OT for text).
> **Status (2026-05-16):** 4,435 Rust LoC · 101 tests · 0 literal-stub markers — gaps are unimplemented features advertised in `lib.rs //!` doc that have no module today.
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (next slice — verified gaps)
- [ ] Add network transport layer (sync protocol does not move bytes today)
  - **Verified gap:** `src/lib.rs:30-36` declares `pub mod coordinator; pub mod crdt; pub mod delta; pub mod error; pub mod merkle; pub mod ot; pub mod vector_clock;` — no `transport` module exists; `Coordinator` operates purely in-process on `DashMap` / `Arc`.
  - **Goal:** A `transport` trait + at least one wire protocol (TCP framed JSON, optionally QUIC via `quinn`) so two `Coordinator` instances on different processes/hosts can exchange CRDT deltas and Merkle diffs.
  - **Design:** `pub trait Transport { async fn send(&self, peer: &DeviceId, msg: SyncMsg) -> SyncResult<()>; async fn recv(&self) -> SyncResult<(DeviceId, SyncMsg)>; }`. Concrete `TcpTransport` with length-prefixed framing (4-byte big-endian length + body). `SyncMsg` enum: `Hello { clock }`, `Delta { delta }`, `MerkleRequest { root }`, `MerkleResponse { branch }`. Body serialized via `serde_json` (later: oxicode).
  - **Files:** (new) `src/transport/mod.rs`, `src/transport/tcp.rs`; rewire `coordinator.rs` to take `Arc<dyn Transport>`.
  - **Tests:** (proposed) `test_tcp_transport_send_recv_roundtrip`, `test_two_coordinators_converge_over_tcp`, `test_partial_send_resilience`.
  - **Risk:** Lifetime / `Send` bounds on the trait — async-trait already pulled in.
  - **Prerequisites:** None.

- [ ] Device discovery via mDNS / DNS-SD (RFC 6762 + RFC 6763)
  - **Verified gap:** `lib.rs` doc claims "Device discovery and state management" (line ~10) but no discovery module exists. `coordinator.rs` requires `DeviceId`s be passed in by the caller.
  - **Goal:** Zero-config LAN discovery — coordinator auto-discovers peers advertising `_oxigeo-sync._tcp.local.` service.
  - **Design:** Use `mdns-sd` crate (Pure Rust). Publish `ServiceInfo { service_type: "_oxigeo-sync._tcp.local.", hostname, port, txt: {device_id, version} }`. Subscribe to same service type; emit `DiscoveryEvent::PeerFound { device_id, addr }`.
  - **Files:** (new) `src/discovery/mod.rs`, `src/discovery/mdns.rs`.
  - **Tests:** (proposed) `test_mdns_publish_subscribe_local`, `test_discovery_event_stream`, `test_txt_record_carries_device_id`.
  - **Risk:** Network-permission required for tests; gate behind `#[ignore]` + explicit invocation in CI.
  - **Prerequisites:** Transport (above) — discovered peers feed `TcpTransport`.

- [ ] CRDT garbage collection — tombstone pruning for `OrSet`
  - **Verified gap:** `src/crdt/or_set.rs` keeps an ever-growing `removed: HashSet<(Element, UniqueTag)>`. No `gc(stable_clock: &VectorClock)` method.
  - **Goal:** Bounded memory for long-running replicas. Once a tombstone is observed by all known devices (per vector clock), drop it.
  - **Design:** `OrSet::gc(stable: &VectorClock)` iterates `removed` and drops entries whose tag-clock < `stable` per-device coordinate. Define "stable" as `min_i clock_i` across all peers tracked by coordinator.
  - **Files:** `src/crdt/or_set.rs` (~80 LoC), `src/coordinator.rs` (`gc_round()` orchestration).
  - **Tests:** (proposed) `test_or_set_gc_drops_stable_tombstones`, `test_or_set_gc_preserves_unobserved`, `test_or_set_gc_idempotent`.
  - **Risk:** Premature GC re-resurrects elements; need conservative `stable` bound.
  - **Prerequisites:** None.

- [ ] Hybrid Logical Clock (HLC) variant of `VectorClock` (Kulkarni et al. 2014)
  - **Verified gap:** `src/vector_clock.rs` provides Lamport-style vector clock only; no wall-clock-merging variant. `lib.rs::Timestamp` is `u64` (logical only).
  - **Goal:** `HybridLogicalClock { physical: u64, logical: u32, device: DeviceId }` with monotonic update on local event and on receive — combines causality with wall-clock readability.
  - **Design:** Algorithm 2 in the HLC paper: on `send` set `(pt', l') = (max(pt, now), if pt' == pt then l+1 else 0)`. On `recv(remote_pt, remote_l)`: `(pt', l') = (max(pt, remote_pt, now), …)`.
  - **Files:** (new) `src/hlc.rs`; re-export from `lib.rs`.
  - **Tests:** (proposed) `test_hlc_local_event_increments`, `test_hlc_receive_advances`, `test_hlc_monotone_under_clock_skew`, `test_hlc_paper_example`.
  - **Risk:** Wall-clock jumps backward — algorithm tolerates this by capping logical drift.
  - **Prerequisites:** None.

- [ ] OR-Set already exists; add **RGA (Replicated Growable Array)** for ordered collections
  - **Verified gap:** `src/crdt/mod.rs:14-17` only re-exports `GCounter, LwwRegister, OrSet, PnCounter`; no ordered/sequence CRDT.
  - **Goal:** Ordered geometry-vertex / feature-list type that converges under concurrent insert/remove. Cite Roh et al. 2011.
  - **Design:** `Rga<T> { elements: BTreeMap<S4Vector, Option<T>> }` where `S4Vector = (lamport_ts, device_id, ssn, seq)`. Insert-after places new element with vector between predecessor and successor.
  - **Files:** (new) `src/crdt/rga.rs`; re-export.
  - **Tests:** (proposed) `test_rga_insert_after_basic`, `test_rga_concurrent_inserts_converge`, `test_rga_remove_then_insert_at_same_pos`, `test_rga_paper_figure_2`.
  - **Risk:** S4Vector comparison is subtle; reference impl available.
  - **Prerequisites:** None.

## Medium Priority
- [ ] Causal-broadcast protocol on top of `Transport`
  - **Goal:** `coordinator.publish(msg)` delivers to all peers respecting causal order from vector clocks (Birman-Joseph-Raeuchle ISIS-style).
  - **Files:** `src/coordinator.rs` (`publish` / `deliver_buffer`).
  - **Why deferred:** Needs transport first.

- [ ] Operational-transform composer extension for **geometry edits**
  - **Goal:** Today `ot::TextOperation` handles strings; add `GeometryOperation::{InsertVertex, RemoveVertex, MoveVertex}` with `transform` + `compose` + `invert` honoring CRDT laws.
  - **Files:** (new) `src/ot/geometry_operation.rs`.
  - **Why deferred:** Text OT is rare in geospatial; RGA above is the bigger win.

- [ ] Conflict visualization helper (serialize divergent states per device)
  - **Goal:** `Coordinator::conflict_report() -> Vec<DivergentField>` for UI.
  - **Files:** `src/coordinator.rs`.
  - **Why deferred:** Needs canonical app-level state to diff against.

- [ ] State-snapshot + bootstrap protocol (fast catch-up for new device)
  - **Goal:** Compress all CRDT state to a single Merkle-rooted blob; new peer fetches once instead of replaying log.
  - **Files:** (new) `src/snapshot.rs`.
  - **Why deferred:** Transport must be live first.

- [ ] Partial-replication by spatial bbox (sync only a region of interest)
  - **Goal:** `SubscribeFilter::Bbox(envelope)` so mobile devices only receive features intersecting their viewport.
  - **Files:** `src/coordinator.rs`, integration with `oxigeo-index` STRtree.
  - **Why deferred:** Spatial-aware CRDTs need RGA + index.

- [ ] Vector-clock compaction for long-running sessions
  - **Goal:** Drop clock entries for devices that have left the group N rounds ago.
  - **Files:** `src/vector_clock.rs`.
  - **Why deferred:** Mostly a memory hygiene item.

- [ ] Sync-protocol authentication (device-identity verification)
  - **Goal:** Each `SyncMsg` signed with device's Ed25519 key; coordinator rejects unknown signers.
  - **Files:** (new) `src/auth/mod.rs`; depends on `ed25519-dalek`.
  - **Why deferred:** Requires transport.

## Low Priority / Future (one-liners)
- [ ] WebRTC data-channel transport for browser-to-browser sync.
- [ ] BFT consensus (HotStuff / PBFT) for critical-metadata replication.
- [ ] Fuzz harness for protocol-level convergence (proptest interleavings).
- [ ] Adaptive batching of delta sends based on RTT.
- [ ] Bridge module for oxigeo-offline ↔ oxigeo-sync (offline-first → P2P sync).
- [ ] Multi-master PostGIS replication adapter using OR-Set tombstones.

## Cross-crate dependencies
- **Blocks:** oxigeo-offline (sync engine plug-in), oxigeo-edge (device-to-device fleet sync), oxigeo-mobile (mobile-app sync layer).
- **Blocked by:** None.

## Recently completed (verbatim)
- (None — existing TODO.md had no `[x]` items.)

---
*Last audited: 2026-05-16*

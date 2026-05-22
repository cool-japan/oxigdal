# TODO: oxigdal-offline

> **Purpose:** Offline-first data management with sync queue, conflict resolution, and optimistic updates for OxiGDAL (SQLite native + IndexedDB WASM; merge strategies; retry; optimistic updates).
> **Status (2026-05-16):** 6,296 Rust LoC · 73 tests · 1 real-stub site (conflict.rs:125 ancestor lookup)
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (next slice — verified gaps)
- [ ] Real three-way merge: implement version-history lookup for `find_common_ancestor`
  - **Verified gap:** `src/conflict.rs:125` — `/// Find common ancestor (placeholder for now)` and body `fn find_common_ancestor(&self, _local: &Record, _remote: &Record) -> Result<Option<Record>> { // In a real implementation, this would query the history / // For now, we don't have version history / Ok(None) }`.
  - **Goal:** Three-way merge (`MergeStrategy::ThreeWayMerge`, declared in `src/merge.rs`) actually has a base version to merge against — today it silently falls through because ancestor is always `None`. Required for non-trivial conflict resolution (Mens 2002).
  - **Design:** Add a `record_versions` table to SQLite backend (`storage/sqlite.rs`) — schema `(record_id, version, parent_version, updated_at, payload)`. On each write, append a row. `find_common_ancestor(local, remote)` does an SQL `WITH RECURSIVE` walk from `local.parent_version` and from `remote.parent_version` until intersecting; LCA = lowest-`version` common row. Mirror in IndexedDB backend (`storage/indexeddb.rs`) using a secondary object store.
  - **Files:** `src/conflict.rs:124-130` (~80 LoC), `src/storage/sqlite.rs` (schema + `get_version_chain`), `src/storage/indexeddb.rs` (mirror), `src/types.rs` (`Record::parent_version: Option<Version>`).
  - **Tests:** (proposed) `test_lca_linear_history`, `test_lca_diamond_concurrent_branches`, `test_lca_disjoint_returns_none`, `test_threeway_merge_uses_ancestor`.
  - **Risk:** Storage migration — bump schema-version + write migration; protect existing-data path with tests.
  - **Prerequisites:** None.

- [ ] Connectivity-detection (online/offline state transitions)
  - **Verified gap:** `src/lib.rs:21` doc claims "Background sync: Automatic sync when connectivity is restored" but `src/sync.rs` `SyncEngine::sync` only checks `remote.ping().await?` lazily — no event-stream of `Online`/`Offline` transitions.
  - **Goal:** `ConnectivityMonitor` emits `Online` / `Offline` events to subscribers; `OfflineManager::set_connectivity_monitor` plugs it in so sync auto-fires on transition.
  - **Design:** Native: polling `ping()` every N seconds OR `tokio::net::TcpStream::connect` heuristic; future hook for OS-level NetworkChangeEvent. WASM: `web_sys::window().navigator().on_line()` + `online` / `offline` events on `window`.
  - **Files:** (new) `src/connectivity.rs` (~150 LoC); plumb into `src/manager.rs` and `src/sync.rs`.
  - **Tests:** (proposed) `test_connectivity_emits_online_event`, `test_connectivity_emits_offline_event`, `test_sync_triggered_on_online_transition`, `test_wasm_online_event_wired` (`#[wasm_bindgen_test]`).
  - **Risk:** Polling cost on mobile — make interval configurable.
  - **Prerequisites:** None.

- [ ] Background-sync worker with configurable interval (currently `sync()` is caller-driven)
  - **Verified gap:** `src/sync.rs::SyncEngine::sync(batch_size)` is callable but no spawned task drives it; the `lib.rs` doc claim "Background sync: Automatic sync when connectivity is restored" relies on this.
  - **Goal:** `OfflineManager::start_background_sync(BackgroundSyncConfig { interval, batch_size, jitter, ... })` spawns a `tokio::task` (native) / `wasm_bindgen_futures::spawn_local` (WASM) that calls `sync()` on a configurable cadence, with backoff on failures.
  - **Design:** `BackgroundSyncWorker { stop_tx, interval, batch_size, jitter }`. Stop via `oneshot` channel. Survives temporary failures by exponential backoff using existing `RetryManager` (`src/retry.rs`).
  - **Files:** `src/manager.rs` (~120 LoC), `src/sync.rs` (worker loop).
  - **Tests:** (proposed) `test_background_worker_runs_periodically`, `test_background_worker_stops_on_signal`, `test_background_worker_backoff_on_failure`.
  - **Risk:** WASM single-thread runtime — keep loop cooperative.
  - **Prerequisites:** Connectivity monitor (above) for the "auto on transition" path.

- [ ] Sync-progress reporting with ETA
  - **Verified gap:** `src/sync.rs` `SyncResult` (struct ~line 80) has `total_operations` and `succeeded` but no streaming progress emitter; `lib.rs` doc lists no progress API.
  - **Goal:** `SyncEngine::sync_with_progress(batch_size, tx: mpsc::Sender<SyncProgress>)` where `SyncProgress = { completed, total, throughput_ops_per_s, eta }`.
  - **Design:** EMA-smoothed throughput from `tokio::time::Instant` deltas; ETA = `(total - completed) / throughput`. Send progress every K ops.
  - **Files:** `src/sync.rs` (~80 LoC), `src/types.rs` (`SyncProgress`).
  - **Tests:** (proposed) `test_progress_reports_monotonically`, `test_progress_final_matches_sync_result`, `test_progress_eta_finite_after_warmup`.
  - **Risk:** Channel backpressure if subscriber is slow; use bounded `mpsc::channel(8)`.
  - **Prerequisites:** None.

- [ ] Delta sync (transfer only changed bytes/fields, not full records)
  - **Verified gap:** `src/sync.rs::push_operation` ships the full `Operation::Update { record }`; no field-level delta is computed even though `Record` is structured.
  - **Goal:** Define `RecordDelta { record_id, version, changed_fields: HashMap<FieldName, FieldValue> }`; producer side computes diff vs. last-shipped version; consumer applies on remote.
  - **Design:** `Record::diff(other: &Record) -> RecordDelta`. For binary blobs, use rsync-style rolling hash; for now (0.1.5) start with structured-field diff only.
  - **Files:** (new) `src/delta.rs`, `src/sync.rs` (wire it), `src/types.rs`.
  - **Tests:** (proposed) `test_delta_field_subset`, `test_delta_apply_idempotent`, `test_delta_no_changes_empty`.
  - **Risk:** Schema evolution — fields renamed on remote will mis-merge; out of scope for 0.1.5.
  - **Prerequisites:** Version history (first item) — delta is computed against ancestor.

## Medium Priority
- [ ] Tile-cache manager for offline map viewing
  - **Goal:** Track downloaded raster tiles, evict by LRU, enforce quota.
  - **Files:** (new) `src/tile_cache.rs`.
  - **Why deferred:** Distinct concern from record-sync; can ship later.

- [ ] Selective sync (choose layers/areas)
  - **Goal:** `SyncFilter::Bbox(envelope)` / `SyncFilter::LayerName(name)` reduces volume on mobile.
  - **Files:** `src/sync.rs`.
  - **Why deferred:** Needs spatial-index hook into `oxigdal-index`.

- [ ] Queue persistence across app restarts
  - **Goal:** SQLite-backed queue already persists records; ensure `Operation` queue is also durable on `clear`/restart.
  - **Files:** `src/queue.rs`, `src/storage/sqlite.rs`.
  - **Why deferred:** Smaller-scope correctness fix; verify with proptest.

- [ ] Bandwidth-aware sync (throttle on metered connections)
  - **Goal:** Reduce batch size when `ConnectivityMonitor::cellular()`.
  - **Files:** `src/sync.rs`.
  - **Why deferred:** Cellular detection needs platform hooks.

- [ ] Conflict-UI helpers (serialize conflict info for display)
  - **Goal:** `Conflict::to_json_for_ui()` returning side-by-side diff.
  - **Files:** `src/conflict.rs`.
  - **Why deferred:** Awaits UI framework integration story.

- [ ] Offline spatial queries via local R-tree index
  - **Goal:** Integrate `oxigdal-index::STRtree` for `query_bbox` on local records.
  - **Files:** (new) `src/spatial.rs`.
  - **Why deferred:** Cross-crate plumbing; defer to 0.2.0.

- [ ] Sync-protocol versioning for backward compat
  - **Goal:** `SyncMsg::version: u16`; reject incompatible peers gracefully.
  - **Files:** `src/sync.rs`.
  - **Why deferred:** Single client today; revisit when external peers exist.

## Low Priority / Future (one-liners)
- [ ] P2P sync without central server via `oxigdal-sync` CRDTs.
- [ ] Offline raster-tile-pyramid generation.
- [ ] Storage-quota mgmt with automatic LRU eviction.
- [ ] Sync analytics (frequency, volume, conflict-rate).
- [ ] At-rest encryption for local SQLite (SQLCipher / age).
- [ ] Multi-user offline collaboration over local mesh.

## Cross-crate dependencies
- **Blocks:** oxigdal-edge (edge sync orchestration), oxigdal-mobile (mobile-app sync layer).
- **Blocked by:** oxigdal-sync (CRDT primitives for P2P path; not blocking 0.1.5 items).

## Recently completed (verbatim)
- (None — existing TODO.md had no `[x]` items.)

---
*Last audited: 2026-05-16*

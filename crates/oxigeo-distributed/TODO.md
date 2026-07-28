# TODO: oxigeo-distributed

> **Purpose:** Distributed processing capabilities for OxiGeo using Apache Arrow Flight (coordinator/worker/Flight RPC; spatial/hash/range partitioning; shuffle).
> **Status (2026-07-28):** 3,830 Rust LoC · 84 tests (all-features and default-features equal) · 0 literal-stub markers — gaps are feature-completeness items advertised in `lib.rs //!` doc that have no wired path.
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (next slice — verified gaps)
- [x] Wire `Coordinator` task scheduling to **real** Flight clients on workers (cross-process tasks)
  - **Verified done:** `src/coordinator.rs` now has `dispatch_task_to_worker(&self, task, worker_id, input: Arc<RecordBatch>) -> Result<TaskResult>` (doc'd as "the end-to-end glue between the three formerly-disconnected components"). It resolves the worker's address, records the assignment, then opens a real `crate::flight::FlightClient::new(address).await?` and calls `client.execute_task(&task, Some(input.as_ref())).await?`, feeding the real response back into `TaskResult::success`/`failure`. Not `serde_json` — uses the typed Flight `execute_task` action directly.
  - **Delta from original design:** method name is `dispatch_task_to_worker`, not `send_task_to_worker`; driven explicitly by the caller with an input `RecordBatch` rather than being called transparently from inside `submit_task`.

- [x] Coordinator failure-detection + worker re-assignment (heartbeat timeout path) — PARTIAL
  - **Verified done:** `src/coordinator.rs::check_worker_timeouts(&self) -> Result<Vec<String>>` reads `worker_timeout_secs` from config, filters workers via `WorkerInfo::is_timed_out(timeout)`, then for each timed-out worker calls `reassign_worker_tasks` (marks its in-flight tasks failed for retry via the scheduler) followed by `remove_worker`. This is real, exercised logic, not a declared-but-unused field.
  - **Gap remaining:** `check_worker_timeouts` is a public method a caller must invoke periodically — there is no self-spawned `tokio::task` watchdog loop inside `Coordinator` calling it automatically every `worker_timeout_secs / 3`. No `tokio::spawn` exists anywhere in `coordinator.rs`. A production deployment must drive this externally (e.g. from the embedding service's own scheduler).

- [ ] Locality-aware partition assignment (today's `Coordinator` is location-blind)
  - **Verified gap:** `src/partition.rs` defines `SpatialExtent`, `TilePartitioner`, `HashPartitioner`, etc., but `Coordinator::submit_task` has no `Partition → WorkerInfo` mapping function that prefers workers whose `WorkerInfo.address` matches the partition's storage locality.
  - **Goal:** `PartitionAssigner` trait with `LocalityAware` impl. Each `Partition` carries optional `data_location_hint: Option<String>` (e.g., S3 region, hostname). Assigner pairs partition → worker by string-similarity / region match; falls back to round-robin.
  - **Design:** Bipartite matching: greedy by region tag, then by least-loaded worker (`active_tasks` field on `WorkerInfo`).
  - **Files:** `src/partition.rs` (~120 LoC) + `src/coordinator.rs` integration.
  - **Tests:** (proposed) `test_locality_aware_prefers_region`, `test_locality_aware_fallback_balanced`, `test_locality_aware_no_hint_round_robin`.
  - **Risk:** Hint format must be stable; document.
  - **Prerequisites:** Coordinator-Flight wiring (above).

- [ ] Shuffle data exchange over network (currently in-process only)
  - **Verified gap:** `src/shuffle.rs::HashShuffle::shuffle(batch)` (referenced in `lib.rs:128`) returns a `Vec<Partition>` in-memory; there is no `network_shuffle` path that ships partitions via `FlightClient::do_put` to peer workers.
  - **Goal:** `NetworkShuffle::execute(batch, worker_addresses)` partitions locally then concurrently `do_put`s each partition to its destination worker keyed by shuffle bucket.
  - **Design:** Bucket = `hash(key) % worker_count`. For each bucket, build a `FlightClient` (cache by address) and call `do_put` with a stream of `FlightData` derived from the partition's `RecordBatch`es.
  - **Files:** (new) `src/shuffle/network.rs` (~150 LoC); export from `src/shuffle.rs`.
  - **Tests:** (proposed) `test_network_shuffle_routes_by_hash`, `test_network_shuffle_recovers_on_one_worker_down`, `test_network_shuffle_preserves_schema`.
  - **Risk:** Worker-side `do_put` handler must accept and accumulate shuffled batches; design dovetails with previous item.
  - **Prerequisites:** Coordinator-Flight wiring + worker `do_put` impl.

- [ ] Spill-to-disk for shuffle buffers exceeding `WorkerConfig::memory_limit`
  - **Verified gap:** `WorkerConfig::memory_limit` (`src/worker.rs:23`) is set but `Worker::execute_task` does not enforce it; in-memory shuffle grows unboundedly.
  - **Goal:** When `current_memory + new_batch_size > memory_limit * 0.8`, serialize oldest in-mem shuffle batch to a tempfile via Arrow IPC (`arrow_ipc::writer::FileWriter`); drain by streaming on consume.
  - **Design:** `SpillManager { dir: PathBuf, spilled: VecDeque<PathBuf>, in_mem_bytes: usize }`. Tempfile via `std::env::temp_dir()` (per CLAUDE.md). On read, `FileReader` decodes; delete after consume.
  - **Files:** (new) `src/spill.rs` (~180 LoC); `src/shuffle.rs` integration.
  - **Tests:** (proposed) `test_spill_triggered_above_threshold`, `test_spill_files_cleaned_up_on_drop`, `test_spill_roundtrip_data_intact`.
  - **Risk:** Disk-full handling; surface `DistributedError::Spill { source }`.
  - **Prerequisites:** None.

## Medium Priority
- [ ] Spatial partitioning via R-tree for predicate pruning
  - **Goal:** Wire `oxigeo-index::STRtree` into `SpatialPartitioner` so coordinator can skip partitions disjoint from the query bbox.
  - **Files:** `src/partition.rs` extension.
  - **Why deferred:** Cross-crate hook; deserves its own slice.

- [ ] Broadcast-join optimization (small table replicated to all workers)
  - **Goal:** `BroadcastShuffle::execute(small_batch)` `do_put`s the small table to every worker once; large-side join is local.
  - **Files:** `src/shuffle.rs` (already exposes `BroadcastShuffle` type).
  - **Why deferred:** Standard query-planner feature; rare workload in geospatial.

- [ ] Per-partition progress reporting (currently coarse `is_complete()`)
  - **Goal:** `Coordinator::partition_progress(id) -> PartitionProgress { state, bytes_processed, eta }`.
  - **Files:** `src/coordinator.rs`.
  - **Why deferred:** Needs streaming task heartbeats from workers.

- [ ] Speculative task execution for straggler mitigation
  - **Goal:** If a task runs > 1.5× median, dispatch a duplicate to an idle worker; winner's result counts.
  - **Files:** `src/coordinator.rs`, `src/task.rs`.
  - **Why deferred:** Requires median-time tracking + duplicate-result reconciliation.

- [ ] Coordinator HA via Raft-style leader election (Ongaro 2014)
  - **Goal:** Standby coordinator takes over on primary failure; uses a small Raft for log replication.
  - **Files:** (new) `src/ha/mod.rs`.
  - **Why deferred:** Major scope; needs a vendored Raft impl decision.

- [ ] Result aggregation with ordered merge (k-way merge for sorted outputs)
  - **Goal:** `Coordinator::collect_results_ordered(by: SortKey)`.
  - **Files:** `src/coordinator.rs`.
  - **Why deferred:** Niche query shape.

- [ ] Task-DAG execution with topological-order dispatch
  - **Goal:** Tasks declare `depends_on: Vec<TaskId>`; coordinator releases dependents once parents complete.
  - **Files:** `src/task.rs` (`TaskScheduler` extension).
  - **Why deferred:** Today's flat-list model suffices for raster pipelines.

## Low Priority / Future (one-liners)
- [ ] Adaptive repartitioning on data-skew detection.
- [ ] Distributed external-merge sort.
- [ ] Pipeline parallelism (overlap I/O / compute / shuffle).
- [ ] Kubernetes-native deployment (pod-per-worker, headless service).
- [ ] Cross-DC WAN-aware scheduling.
- [ ] Lineage tracking for provenance.
- [ ] Distributed GeoTIFF mosaic assembly.

## Cross-crate dependencies
- **Blocks:** oxigeo-cluster (uses oxigeo-distributed as compute substrate), oxigeo-services (would expose a "/jobs" endpoint backed by Coordinator).
- **Blocked by:** None (Arrow Flight, tonic, arrow-flight already in `Cargo.toml`).

## Recently completed (verbatim)
- (None — existing TODO.md had no `[x]` items.)

---
*Last audited: 2026-07-28 (status line refreshed: 38→84 tests, LoC 3,933→3,830, date bumped; Flight task dispatch and worker-timeout reassignment confirmed real and flipped to done — the latter marked PARTIAL since it still needs an external caller to poll it, no self-spawned watchdog task; locality-aware assignment, network shuffle, and spill-to-disk re-checked and confirmed still absent — left open)*

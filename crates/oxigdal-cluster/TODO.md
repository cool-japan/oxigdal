# TODO: oxigdal-cluster

> **Purpose:** Distributed orchestration for OxiGDAL — task graph, work-stealing scheduler, worker pool, Raft-based coordinator, distributed cache (coherency), replication, fault tolerance (circuit breaker, bulkhead, health checks), autoscaler, workflow engine, monitoring, security/RBAC.
> **Status (2026-05-16):** 11,394 LoC · 90 tests · 5 surfaced "simulated / simplified" sites (Raft `request_votes` in-process, scheduler/advanced worker selection, scheduler/advanced batch-fit, coordinator log update, network compression simplified) — overall: skeleton with most network paths conceptual.
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Real network transport for inter-node communication (gRPC over tonic)
  - **Verified gap:** `Cargo.toml:62` — `# Note: Raft-based consensus is planned for a future release`; `src/coordinator.rs:421-422` — `// In a real implementation, this would send vote requests to other nodes / // For now, simulate by checking cluster size`. Per project memory the cluster crate has "scheduler, work-stealing, autoscaler" as roadmap items.
  - **Goal:** A working tonic-gRPC transport between coordinator and workers, plus worker-to-worker for cache + work-steal. Proto schema for `RequestVote`, `AppendEntries`, `Heartbeat`, `StealRequest`, `CacheGet`/`CachePut`.
  - **Design:** New `src/network/proto/cluster.proto` compiled via `tonic-build` in `build.rs`; `ClusterCoordinatorServer` implements the trait; clients are stored in a `DashMap<NodeId, Channel>` reused via tonic's connection pooling. TLS via `rustls` (matches workspace TLS stack — note RUSTSEC advisories listed in MEMORY.md are accepted).
  - **Files:** `crates/oxigdal-cluster/proto/cluster.proto` (new), `build.rs` (new), `src/network/transport.rs` (new), `src/coordinator.rs:419-433` (replace stub `request_votes`).
  - **Tests:** (proposed) `test_transport_three_node_request_vote_majority`, `test_transport_heartbeat_keeps_session_alive`, `test_transport_partition_triggers_election`, `test_transport_tls_handshake_succeeds`, `test_transport_back_pressure_when_queue_full`.
  - **Risk:** `build.rs` invocation order under workspace builds — verify nextest still discovers tests; or generate code committed under `src/network/generated.rs` (no build script).
  - **Prerequisites:** None — `tonic`, `prost`, `arrow-flight` already in `Cargo.toml`.

- [ ] Raft consensus log persistence to disk
  - **Goal:** Replace the in-memory `LogEntry` vec inside `ClusterCoordinator` with a write-ahead-log on disk; on restart the coordinator replays its log and recovers term/voted-for. Per Diego Ongaro's Raft paper (2014, "In Search of an Understandable Consensus Algorithm", USENIX ATC) §5.4 the log MUST be persisted before responding to RPCs.
  - **Design:** Append-only segmented log files under `<data_dir>/raft/{log,state}/`, each segment max 64 MiB, fsynced on append; index file maps `term:index -> file_offset`. Use `oxiarc-zstd` (already a dep) only for snapshots, never for the active log. Persist `current_term` and `voted_for` atomically via rename of `.tmp` files.
  - **Files:** `crates/oxigdal-cluster/src/coordinator/wal.rs` (new), `src/coordinator.rs` (wire WAL on every state mutation).
  - **Tests:** (proposed) `test_wal_persist_then_recover_log_indices`, `test_wal_fsync_before_ack`, `test_wal_segment_roll_at_64mib`, `test_wal_corrupt_segment_truncated_to_last_good_offset`, `test_wal_voted_for_atomic_rename`.
  - **Risk:** fsync cost on slow disks — provide `RaftPersistenceMode::{Strict, Buffered}` and document data-loss implications of Buffered.
  - **Prerequisites:** Item 1 (transport) for end-to-end leader-election test.

- [ ] Real work-stealing protocol over the network
  - **Verified gap:** `src/scheduler/advanced.rs:177` — `// Simplified worker selection - in production, this would check actual capacities`; `:626` `// For now, assume small tasks fit`.
  - **Goal:** Idle worker calls `StealRequest { from_worker_id, max_tasks }` to a randomly chosen peer; peer dequeues up to `max_tasks` from the tail of its task buffer (Cilk-style THE protocol per Frigo, Leiserson & Randall 1998) and ships them.
  - **Design:** Add `WorkStealingClient` that periodically (every 50ms when local queue empty) selects a target via `Power-of-two-choices`; uses the cluster.proto transport (Item 1) to fetch tasks. Real capacity check: query `WorkerCapacity` reported in heartbeats.
  - **Files:** `crates/oxigdal-cluster/src/scheduler/work_stealing.rs` (new), `src/scheduler/advanced.rs:172-200` (replace stubbed `find_workers_for_gang`).
  - **Tests:** (proposed) `test_steal_when_local_queue_empty`, `test_steal_respects_max_tasks_cap`, `test_steal_zero_when_target_also_empty`, `test_steal_capacity_check_rejects_overloaded_target`, `test_steal_random_target_selection_power_of_two`.
  - **Risk:** Steal-thrash under network partition — apply exponential backoff after 3 consecutive empty steals.
  - **Prerequisites:** Item 1 (transport).

- [ ] Task serialization for network transmission
  - **Goal:** Define a Pure-Rust serialization for `Task` so it can travel across nodes. Use `oxicode` (CBOR per COOLJAPAN replacement for bincode); embed a schema-version byte at offset 0.
  - **Design:** `Task` already derives `Serialize`/`Deserialize` (via workspace `serde`); add `Task::to_wire() -> Vec<u8>` / `Task::from_wire(&[u8]) -> Result<Self>` that prepend a `(magic: [u8; 4], version: u8)` header. Reject unknown versions with a clear error.
  - **Files:** `crates/oxigdal-cluster/src/task_graph.rs` (extend `Task` impl), new `src/network/wire.rs`.
  - **Tests:** (proposed) `test_task_wire_roundtrip`, `test_task_wire_unknown_version_errors`, `test_task_wire_size_under_4kb_for_typical`, `test_task_wire_payload_blob_preserved`.
  - **Risk:** Schema evolution — keep header stable; never replace `oxicode` with bincode (COOLJAPAN policy).
  - **Prerequisites:** None.

- [ ] Checkpoint persistence to durable storage
  - **Goal:** `FaultToleranceManager::checkpoint(task_id, state) -> Result<()>` writes to `<data_dir>/checkpoints/<task_id>.ckpt`, with content-addressed naming for dedup and `oxiarc-zstd` compression for large states (>4 KiB).
  - **Design:** Atomic-rename pattern (`tmp → final`); SHA-256 over content; manifest JSON tracks `task_id → ckpt_hash`. On task resume, load by id and verify hash; on hash mismatch, return `CheckpointCorrupted`.
  - **Files:** `crates/oxigdal-cluster/src/fault_tolerance/checkpoint.rs` (new).
  - **Tests:** (proposed) `test_checkpoint_save_atomic_via_rename`, `test_checkpoint_loaded_state_matches_saved`, `test_checkpoint_zstd_compression_above_4kib`, `test_checkpoint_dedup_identical_content`, `test_checkpoint_corrupted_hash_errors`.
  - **Risk:** None significant — pure local I/O.
  - **Prerequisites:** None.

- [ ] Autoscaler wired to cloud-provider APIs (AWS ASG, GCP MIG, Azure VMSS)
  - **Goal:** Replace local-only scaling decisions with real API calls; expose pluggable `CloudProvider` trait so each provider's SDK can be a separate optional feature.
  - **Design:** Trait `CloudProvider { async fn scale_to(&self, n: usize) -> Result<()>; async fn current_instances(&self) -> Result<Vec<InstanceId>>; }`. Implementations: `aws_asg::AwsAutoscaler` (feature `aws`), `gcp_mig::GcpAutoscaler` (feature `gcp`), `azure_vmss::AzureAutoscaler` (feature `azure`). All HTTP via `reqwest` (already a workspace dep). Provider credentials from env or IAM-role metadata.
  - **Files:** `crates/oxigdal-cluster/src/autoscale/` extend with new submodules.
  - **Tests:** (proposed) `test_aws_asg_scale_request_signed`, `test_gcp_mig_set_target_size`, `test_azure_vmss_scale_capacity`, `test_autoscale_dispatcher_picks_correct_provider`, `test_autoscale_falls_back_when_provider_unconfigured`.
  - **Risk:** Each cloud SDK adds significant dep weight — strictly opt-in via features.
  - **Prerequisites:** None.

- [ ] Cluster-membership protocol (SWIM or gossip)
  - **Goal:** Node discovery and failure detection without central coordinator dependency. SWIM (Das, Gupta, Motivala 2002, DSN '02) is the canonical pick: probe → indirect-probe → suspect → confirm.
  - **Design:** Each node sends a `Ping` every 1s to a random peer; if no `Ack`, send `IndirectPing` to k peers asking them to ping the suspect; on continued silence mark `Suspect`; after `suspicion_timeout`, mark `Dead` and broadcast `MemberUpdate`. Piggyback `MemberUpdate`s on regular Ping/Ack messages.
  - **Files:** `crates/oxigdal-cluster/src/network/swim.rs` (new ~600 LoC), `src/coordinator.rs` (integrate `MemberStatus` updates).
  - **Tests:** (proposed) `test_swim_direct_ping_marks_alive`, `test_swim_indirect_ping_recovers_false_suspect`, `test_swim_dead_after_suspicion_timeout`, `test_swim_member_update_propagates`, `test_swim_partition_split_brain_resolved_after_heal`.
  - **Risk:** SWIM convergence time is O(log N) — document for clusters > 100 nodes.
  - **Prerequisites:** Item 1 (transport).

## Medium Priority
- [ ] Distributed cache invalidation over network (currently local simulation; couples with Item 1).
  - **Files:** `src/cache_coherency.rs`.
  - **Why deferred:** Needs transport (Item 1).
- [ ] Replication data transfer between replicas over TCP/gRPC.
  - **Files:** `src/replication.rs`.
  - **Why deferred:** Needs transport.
- [ ] Speculative execution with result deduplication.
  - **Files:** `src/scheduler/speculative.rs` (new).
  - **Why deferred:** Optimization on top of basic scheduling.
- [ ] Resource-quota enforcement across distributed workers.
  - **Files:** `src/resources/` (extend).
  - **Why deferred:** Needs cluster-wide accounting via transport.
- [ ] Gang scheduling for tightly-coupled geospatial ops.
  - **Files:** `src/scheduler/gang.rs` (replaces `find_workers_for_gang` stub at `scheduler/advanced.rs:172-200`).
  - **Why deferred:** After basic scheduling is network-real.
- [ ] Topology-aware scheduling using real latency measurements.
  - **Files:** `src/data_locality.rs`.
  - **Why deferred:** Need RTT probe via transport.
- [ ] Workflow-engine persistence (resume after coordinator restart).
  - **Files:** `src/workflow/persistence.rs` (new).
  - **Why deferred:** Pairs with Raft WAL (Item 2).
- [ ] Alert delivery (email, Slack webhook, PagerDuty).
  - **Files:** `src/monitoring/alerts.rs` (extend).
  - **Why deferred:** Out-of-process integration; not blocking core scheduling.
- [ ] RBAC policy enforcement at task submission.
  - **Files:** `src/security/rbac.rs` (extend).
  - **Why deferred:** Existing `SecurityManager` covers basics.

## Low Priority / Future (one-liners)
- [ ] Kubernetes operator for cluster-lifecycle management.
- [ ] Multi-cluster federation for geo-distributed processing.
- [ ] GPU resource scheduling for ML inference tasks (coordinate with oxigdal-gpu).
- [ ] Priority preemption (evict low-priority tasks for urgent ones).
- [ ] Cost-aware scheduling (spot / preemptible instances).
- [ ] Cluster state snapshots for disaster recovery.
- [ ] Built-in benchmarking suite for cluster-performance profiling.

## Cross-crate dependencies
- **Blocks:** oxigdal-services (distributed services), oxigdal-distributed (parent crate).
- **Blocked by:** tonic (transport), rustls (TLS — accepted RUSTSEC advisories per MEMORY.md), oxicode (wire encoding), oxiarc-zstd (checkpoint compression).

## Recently completed (verbatim)
*(No `[x]` entries on previous TODO.)*

---
*Last audited: 2026-05-17*

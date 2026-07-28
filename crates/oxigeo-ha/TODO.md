# TODO: oxigeo-ha

> **Purpose:** High availability, disaster recovery, and automatic failover for OxiGeo — active-active replication, Raft-style leader election, PITR/WAL recovery, multi-site DR.
> **Status (2026-07-28):** 5,171 LoC (src, as of last count) · 93 tests (all-features) · 3 of the 4 simulated/placeholder code paths from the 2026-05-16 audit are now real (network transport, WAL replay, Raft election); concrete HTTP/TCP health probes remain open below.
> **Roadmap:** v0.1.7 → v0.2.1 → v1.0.0

## High Priority (verified gaps)
- [x] Replace simulated network send in active-active replication with a real transport.
  - **Done:** The `sleep(10ms)` simulate block is gone. `src/replication/active_active.rs` now defines a real `ReplicaTransport` trait (imported from `src/replication/transport.rs` as `EventApplier`/`EventReceiver`/`ReplicaTransport`), stored as `transport: Arc<RwLock<Option<Arc<dyn ReplicaTransport>>>>` and set via `set_transport()`. `send_to_replica` requires a configured transport and returns an error (marking the replica `Failed`) instead of fabricating success when none is set — statistics/lag are only updated after a real acknowledgment.
  - **Files:** `src/replication/transport.rs`, `src/replication/active_active.rs`.

- [x] Real WAL replay for point-in-time recovery (currently sleeps 100 ms and returns fake count).
  - **Done:** `replay_wal_to_time` (`src/recovery/pitr.rs`) no longer sleeps and fakes `1000u64`. It now requires a configured `applier()` (erroring rather than "refusing to report a fabricated replay count" if absent), reads real WAL entries via `self.wal.read_entries()` (checksum-verified and LSN/commit-ordered), re-verifies each entry's checksum before applying, and returns the genuine count of entries at-or-before `target_time` (logging a warning, not an error, when that count is honestly zero).
  - **Files:** `src/recovery/pitr.rs`, `src/recovery/wal.rs`.

- [x] Implement multi-node Raft vote collection in `LeaderElection::start_election` (currently counts only self-vote).
  - **Done:** `src/failover/election.rs` no longer computes `majority` from `votes_received.len()` alone. `start_election` now computes `total_nodes = peers.len() + 1`, `majority = (total_nodes / 2) + 1`, and actively broadcasts a real `VoteRequest` to every known peer before checking `votes_received.len() >= majority`; `handle_vote_request` implements the follower side of the exchange. This is real multi-node quorum collection, not a trivially-self-winning election.
  - **Files:** `src/failover/election.rs`.

- [ ] Real HTTP/TCP health-check probes in `healthcheck/checks.rs` (currently structure only).
  - **Re-verified 2026-07-28 — partially evolved, core gap remains:** `src/healthcheck/checks.rs` (352 LoC) grew a pluggable `trait DependencyProbe { async fn probe(&self) -> HaResult<()>; }` plus real `LivenessSignal`/`ReadinessGate`/`LivenessCheck`/`ReadinessCheck`/`DependencyCheck` types (heartbeat staleness, readiness gating), which is a legitimate step and mirrors the fail-closed `CustomProbe` pattern used in `oxigeo-gateway`. However there is still no concrete built-in `HttpCheck`/`TcpCheck` implementation — no `reqwest` or `tokio::net::TcpStream` usage in this file — so callers must supply their own `DependencyProbe`; the crate ships no ready-to-use network prober.
  - **Goal:** `HttpCheck` issues GET to endpoint, classifies status; `TcpCheck` opens TCP connection within timeout; results feed `HealthCheckAggregator`.
  - **Design:** `reqwest::Client` shared, `tokio::net::TcpStream::connect` with `tokio::time::timeout`. Expose `latency_ms` + `error_message`. Compose via the new `DependencyProbe` trait so they plug into `LivenessCheck`/`DependencyCheck` directly.
  - **Files:** `src/healthcheck/checks.rs`.
  - **Tests:** (proposed) `test_http_check_returns_latency`, `test_http_check_5xx_marks_unhealthy`, `test_tcp_check_timeout`, `test_aggregator_quorum_weighting`.
  - **Risk:** Network egress in tests — use `wiremock`.
  - **Prerequisites:** None.

## Medium Priority
- [ ] Raft AppendEntries (log replication) protocol on top of leader election.
  - **Goal:** After leader election succeeds, replicate `LogEntry` records via `AppendEntriesRequest` per Raft §5.3.
  - **Files:** New `src/failover/log_replication.rs`.
  - **Why deferred:** Election (item 3 in High) must land first; AppendEntries is a separate phase.

- [ ] Snapshot-based backup to cloud object storage (S3/GCS/Azure Blob via `oxigeo_core::ObjectStore`).
  - **Files:** `src/backup/full.rs`, `src/backup/incremental.rs` (currently 61 + 79 LoC of scaffolding).

- [ ] Cross-region DR runbook executor (failover → DNS switch → traffic redirect).
  - **Files:** `src/dr/` (untouched scaffolding).

- [ ] Split-brain detection + resolution for active-active clusters.
  - **Goal:** Quorum-based fencing using `current_leader` consensus; lower-term node steps down on contact.
  - **Files:** `src/conflict/strategies.rs` (CRDT framework exists; integrate).

- [ ] Replication lag monitoring with configurable alert thresholds.
  - **Files:** `src/replication/lag_monitor.rs:361 LoC` (already exists; needs integration with active-active path).

- [ ] Connection draining during planned failover (in-flight requests complete before promotion).
  - **Files:** `src/failover/client_redirect.rs`.

- [ ] Backup verification (restore to ephemeral instance + integrity check).

- [ ] Read replicas with configurable consistency (eventual / strong via vector-clock check).

## Low Priority / Future (one-liners)
- [ ] CRDT-based multi-region active-active (state-based: G-Counter, OR-Set; op-based registers).
- [ ] Blue-green deployment with automated rollback.
- [ ] Canary deployment with progressive traffic shifting.
- [ ] Chaos-engineering injection hooks (fault, latency, partition).
- [ ] HA SLA compliance reporting (uptime %, MTTR, MTBF).
- [ ] Geo-fenced data-residency replication.
- [ ] Automated capacity planning from failover history.

## Cross-crate dependencies
- **Blocks:** `oxigeo-cluster` (consumes Raft + replication), `oxigeo-server` (consumes failover hooks).
- **Blocked by:** None.

## Recently completed (verbatim)
*No prior `[x]` entries as of 2026-05-16; this 2026-07-28 audit found the real replication transport, WAL replay, and multi-node Raft election already implemented in source.*

---
*Last audited: 2026-07-28*

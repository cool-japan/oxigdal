# TODO: oxigeo-ha

> **Purpose:** High availability, disaster recovery, and automatic failover for OxiGeo — active-active replication, Raft-style leader election, PITR/WAL recovery, multi-site DR.
> **Status (2026-05-16):** 5,171 LoC (src) · 46 tests (32 inline + 14 in tests/) · 4 simulated/placeholder code paths
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Replace simulated network send in active-active replication with a real transport.
  - **Verified gap:** `src/replication/active_active.rs:201-203` —
    ```rust
    // Simulate network send
    // In a real implementation, this would send over network
    sleep(Duration::from_millis(10)).await;
    ```
  - **Goal:** Replicate `ReplicationEvent` batches over a real transport (TCP + length-prefixed `oxicode`-serialized frames, or gRPC). Receiver feeds `receive_event` (already implemented at `active_active.rs:222`).
  - **Design:** Define `trait ReplicationTransport { async fn send(&self, peer: Uuid, batch: EventBatchMessage) -> HaResult<AckMessage>; async fn recv(&self) -> HaResult<EventBatchMessage>; }`. Provide `TcpTransport` (tokio) and `InMemoryTransport` (current behaviour, gated for tests). Frame format: `[u32 LE length][oxicode bytes]`. Optional LZ4/Zstd compression per `CompressionAlgorithm` enum already declared in `protocol.rs:14`.
  - **Files:** New `src/replication/transport.rs`; modify `src/replication/active_active.rs:180-220` (replace simulate block); `src/replication/mod.rs` (re-export trait).
  - **Tests:** (proposed) `test_tcp_transport_roundtrip`, `test_transport_handshake_version_check`, `test_transport_ack_with_vector_clock`, `test_transport_zstd_compression`, `test_two_node_replication_apply_event`.
  - **Risk:** Need reconnection/backoff logic — defer to `lag_monitor.rs` integration. Vector clock ordering already implemented in `protocol.rs`; transport just moves bytes.
  - **Prerequisites:** None.

- [ ] Real WAL replay for point-in-time recovery (currently sleeps 100 ms and returns fake count).
  - **Verified gap:** `src/recovery/pitr.rs:85-91` —
    ```rust
    async fn replay_wal_to_time(&self, target_time: DateTime<Utc>) -> HaResult<u64> {
        info!("Replaying WAL to time: {}", target_time);
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let transactions_replayed = 1000u64;
        Ok(transactions_replayed)
    }
    ```
    Also `RecoveryTarget::TransactionId(_)` returns `HaError::NotImplemented` at `pitr.rs:54`.
  - **Goal:** Iterate WAL segments under `data_dir`, deserialize entries, apply each whose `commit_ts <= target_time`. Returns real `transactions_replayed`. Implement `TransactionId` target by stopping at matching LSN.
  - **Design:** WAL file format: `[u64 LE LSN][u64 LE timestamp_ms][u32 LE len][oxicode WalEntry]` framed records, with CRC32 trailer (`crc32fast` already in deps). Segment files: `wal-<startLSN>.log` rotated at configured size. `WalReader::iter()` yields entries in LSN order; recovery filters by timestamp. Use checksum to detect torn writes; stop at first invalid CRC.
  - **Files:** New `src/recovery/wal.rs` (~400 LoC); modify `src/recovery/pitr.rs:43-92`.
  - **Tests:** (proposed) `test_wal_writer_reader_roundtrip`, `test_pitr_replay_stops_at_timestamp`, `test_pitr_transaction_id_target_resolves_lsn`, `test_wal_torn_write_detection`, `test_wal_segment_rotation`.
  - **Risk:** Without a real WAL writer in the rest of OxiGeo, this is structurally a library — tests will use a fixture writer. Document this as "transport layer; consumers must call `WalWriter::append`".
  - **Prerequisites:** None.

- [ ] Implement multi-node Raft vote collection in `LeaderElection::start_election` (currently counts only self-vote).
  - **Verified gap:** `src/failover/election.rs:165-171` —
    ```rust
    sleep(election_timeout).await;
    let votes_count = self.votes_received.len();
    let total_nodes = votes_count;
    let majority = (total_nodes / 2) + 1;
    let won = votes_count >= majority;
    ```
    With only `self.votes_received.insert(self.node_id, self_vote)` at line 158, `votes_count == 1` always → `majority = 1` → always wins. Trivially-passing election. The doc comment at `election.rs:1` says "Leader election implementation (Raft-based)" — name not implementation.
  - **Goal:** Real Raft (Ongaro & Ousterhout 2014, USENIX ATC) RequestVote RPC round: broadcast `VoteRequest` to all known peers, collect `VoteResponse` until quorum or timeout, follower behaviour per term, randomized election timeout.
  - **Design:** Use the transport from item 1 to send `VoteRequest { candidate_id, term, last_log_index, last_log_term }` to every peer in cluster membership. Each peer (via `handle_vote_request` at `election.rs:202+`) responds; collect into `votes_received` until `len() >= (peers.len() / 2) + 1`. Randomize timeout in [election_timeout_ms, 2 × election_timeout_ms] per Raft §5.2 to avoid split votes. Track `commit_index`/`last_applied` for log catch-up after promotion.
  - **Files:** `src/failover/election.rs:139-203` (rewrite `start_election`); add `peers: Arc<DashMap<Uuid, PeerInfo>>` field; new `src/failover/log.rs` for Raft log (separate concern, can be a stub initially with just term/index).
  - **Tests:** (proposed) `test_election_wins_with_majority`, `test_election_loses_without_majority`, `test_election_higher_term_demotes_to_follower`, `test_election_split_vote_retries_with_new_term`, `test_concurrent_candidates_one_wins`, `test_election_with_3_node_cluster`.
  - **Risk:** Largest item in the crate; must coordinate with item 1 (transport). Without AppendEntries (log replication) this is leader-election-only; add a separate medium-priority item for log replication.
  - **Prerequisites:** Item 1 (transport).

- [ ] Real HTTP/TCP health-check probes in `healthcheck/checks.rs` (currently structure only).
  - **Goal:** `HttpCheck` issues GET to endpoint, classifies status; `TcpCheck` opens TCP connection within timeout; results feed `HealthCheckAggregator`.
  - **Design:** `reqwest::Client` shared, `tokio::net::TcpStream::connect` with `tokio::time::timeout`. Expose `latency_ms` + `error_message`. Compose into existing `aggregator.rs` weighted scoring.
  - **Files:** `src/healthcheck/checks.rs` (158 LoC currently; flesh out).
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
*No prior `[x]` entries — slate was empty.*

---
*Last audited: 2026-05-16*

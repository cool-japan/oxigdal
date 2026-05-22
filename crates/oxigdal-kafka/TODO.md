# TODO: oxigdal-kafka

> **Purpose:** Apache Kafka integration for OxiGDAL — async producer/consumer with schema registry and transactions.
> **Status (2026-05-16):** 5,749 LoC · 149 tests · 1 real-code stub + 1 dep-policy violation
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Replace `rdkafka` C/C++ FFI dependency with a Pure Rust Kafka client (COOLJAPAN Pure Rust Policy violation)
  - **Verified gap:** `Cargo.toml:50` — literal: `rdkafka = { workspace = true, features = ["tokio"], default-features = false }`
  - **Goal:** Drop `librdkafka` (C/C++ via `cmake-build` feature on line 28) and ship a Pure Rust wire-protocol implementation under `src/protocol/`. Default `producer` + `consumer` features must build with zero C/C++ deps.
  - **Design:** Implement Kafka protocol v2+ framing (4-byte length + ApiKey + ApiVersion + CorrelationId + ClientId + Body) per the Apache Kafka protocol guide (kafka.apache.org/protocol). Required APIs for v0.2.0: ApiVersions (key 18), Metadata (3), Produce (0), Fetch (1), ListOffsets (2), FindCoordinator (10), JoinGroup (11), SyncGroup (14), Heartbeat (12), LeaveGroup (13), OffsetCommit (8), OffsetFetch (9), SaslHandshake (17), SaslAuthenticate (36). Use `bytes::Buf/BufMut` for zero-copy varint / compact-string encoding. Wire `producer/mod.rs` and `consumer/mod.rs` to a `kafka_client::Conn` instead of `rdkafka::FutureProducer` / `StreamConsumer`. Re-validate compression bindings against `oxiarc-snappy`, `oxiarc-lz4`, `oxiarc-zstd` (already in `Cargo.toml`).
  - **Files:** new `src/protocol/{mod.rs,frame.rs,api_versions.rs,produce.rs,fetch.rs,metadata.rs,group.rs,sasl.rs}`; rewrite `src/producer/mod.rs:18-19,42-97` (currently `use rdkafka::...`); rewrite `src/consumer/mod.rs:22-25,36-50` (currently `use rdkafka::config::RDKafkaLogLevel; ... consumer: Arc<StreamConsumer<CustomContext>>`).
  - **Tests:** (proposed) `test_apiversions_roundtrip`, `test_produce_v9_record_batch`, `test_fetch_v11_compressed`, `test_metadata_v9_discovery`, `test_sasl_plain_handshake`, `test_consumer_group_join_sync_heartbeat`, `test_replaces_rdkafka_zero_c_deps_check`.
  - **Risk:** Kafka protocol negotiates per-API versions independently; matching librdkafka coverage is large work. Plan staged release: v0.1.5 keeps rdkafka but ships protocol skeleton; v0.2.0 makes Pure Rust the default and feature-gates rdkafka behind `legacy-rdkafka`.
  - **Prerequisites:** None.

- [ ] Implement SASL PLAIN and SCRAM-SHA-256/512 authentication handshake (currently config-only stub)
  - **Verified gap:** `src/config.rs:120-127` — literal: `/// Convert to rdkafka SASL mechanism string` followed by string returns `"PLAIN"`, `"SCRAM-SHA-256"`, `"SCRAM-SHA-512"`, `"GSSAPI"` — values forwarded to librdkafka, never implemented in-crate.
  - **Goal:** Native SASL exchange against Kafka brokers without delegating to librdkafka. PLAIN per RFC 4616; SCRAM-SHA-256/512 per RFC 5802 / RFC 7677.
  - **Design:** New `src/protocol/sasl.rs`. PLAIN: `\0<authzid>\0<authcid>\0<passwd>` UTF-8, single frame, encoded in `SaslAuthenticate` request. SCRAM: client-first → server-first → client-final → server-final per RFC 5802; HMAC-SHA-{256,512} via `sha2` + `hmac` (already on workspace), PBKDF2-HMAC via `pbkdf2` crate. Use channel-binding `n,,` (no channel binding) until TLS-binding spec finalised. Wire into a new `sasl_authenticate(conn, mechanism, creds)` called after `SaslHandshake` (ApiKey 17).
  - **Files:** new `src/protocol/sasl.rs`; modify `src/config.rs:108-127` (replace `to_rdkafka_str` with `mechanism_token()`).
  - **Tests:** (proposed) `test_sasl_plain_initial_message_format` (RFC 4616 §2 exact bytes), `test_scram_sha256_client_first_message_bare`, `test_scram_sha256_client_proof_against_rfc7677_vector`, `test_scram_sha512_full_exchange_kafka_quickstart_vector`.
  - **Risk:** GSSAPI/Kerberos out of scope for v0.2.0 (requires `libgssapi`, C dep). Document explicitly.
  - **Prerequisites:** Pure Rust wire-protocol framing (item above).

## Medium Priority
- [ ] Implement idempotent producer (producer ID + sequence numbers per partition)
  - **Goal:** Avoid duplicate messages on retry via Kafka's idempotent producer protocol (KIP-98).
  - **Files:** `src/producer/mod.rs`, new `src/producer/idempotence.rs`.
  - **Why deferred:** Requires `InitProducerId` (ApiKey 22) and per-partition sequence tracking — depends on the Pure Rust protocol layer landing first.

- [ ] Implement transactional producer (init / begin / commit / abort) for exactly-once
  - **Goal:** Bring `src/transactions/coordinator.rs:36-150` from local state-machine to broker-coordinated transactions per KIP-98 / KIP-129.
  - **Files:** `src/transactions/coordinator.rs`, `src/transactions/producer.rs`.
  - **Why deferred:** Requires `InitProducerId` (22), `AddPartitionsToTxn` (24), `AddOffsetsToTxn` (25), `EndTxn` (26), `TxnOffsetCommit` (28); blocked by protocol layer.

- [ ] Implement partition assignment strategies (Range, RoundRobin, Sticky / KIP-54)
  - **Goal:** Pluggable `Assignor` trait used during `JoinGroup` / `SyncGroup`.
  - **Files:** new `src/consumer/assignor.rs`.
  - **Why deferred:** Currently `rdkafka` selects internally via config string.

- [ ] Add TLS encryption support via `rustls` (replace `rdkafka`'s OpenSSL path)
  - **Goal:** `tokio-rustls` adapter for `SaslSsl` / `Ssl` `SecurityProtocol` variants.
  - **Files:** new `src/protocol/tls.rs`.
  - **Why deferred:** Blocked by protocol layer; until then `rdkafka` handles TLS via OpenSSL (C dep).

- [ ] Producer record batching with `linger.ms` / `batch.size` semantics
  - **Goal:** Native batching to replace `rdkafka`'s internal queue.
  - **Files:** `src/producer/batch.rs` (already has scaffold at 243 LoC).
  - **Why deferred:** Becomes meaningful once Pure Rust protocol path is wired.

- [ ] Avro schema-registry compatibility checks (BACKWARD / FORWARD / FULL / NONE per Confluent compatibility matrix)
  - **Goal:** Full evolution rules in `src/schema_registry/compatibility.rs` (220 LoC; rules currently partial).
  - **Files:** `src/schema_registry/compatibility.rs`.
  - **Why deferred:** Requires schema-resolution edge-case suite (default values, union promotion, enum aliases).

- [ ] Geospatial-aware partitioner (geohash- or tile-coord-keyed)
  - **Goal:** Custom `Partitioner` trait impl that hashes by geohash prefix for spatial locality.
  - **Files:** `src/producer/partitioner.rs` (already has `Partitioner` trait at 239 LoC).
  - **Why deferred:** Domain-specific; awaiting concrete user.

## Low Priority / Future (one-liners)
- [ ] Admin client (CreateTopics, DeleteTopics, AlterConfigs, DescribeCluster)
- [ ] Kafka Streams-like DSL for streaming geospatial transforms
- [ ] Two-phase commit across Kafka and external sinks for cross-system exactly-once
- [ ] Dead-letter topic auto-routing for poison-pill messages
- [ ] Header-based message routing for multi-tenant tile pipelines
- [ ] Consumer lag monitoring (compare committed vs log-end offset)

## Cross-crate dependencies
- **Blocks:** oxigdal-streaming (durable sink), oxigdal-services (event push)
- **Blocked by:** oxiarc-snappy, oxiarc-lz4, oxiarc-zstd (already integrated for compression features)

## Recently completed (verbatim)
- *(none in this slice)*

---
*Last audited: 2026-05-16*

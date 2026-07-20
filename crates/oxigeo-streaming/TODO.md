# TODO: oxigeo-streaming

> **Purpose:** Real-time data processing and streaming pipelines for OxiGeo — windowing, watermarking, transformations, stateful operators, cloud object-store I/O.
> **Status (2026-05-16):** 17,383 LoC · 617 tests · 2 real-code stubs
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Wire `XyzProtocol::get_tile` to an actual HTTP fetch instead of returning empty bytes (completed 2026-05-17)
  - **Done:** `XyzProtocol::get_tile` now performs real HTTP GET via `reqwest::Client` (feature-gated `tile-http`). URL constructed via `XyzUrlTemplate::expand`. 404 → `TileNotFound`; 5xx → exponential back-off retry (3 attempts, 100ms initial). `get_tile_metadata` extracts `ETag`, `Last-Modified`, `Content-Length` headers. `tile-http` feature added to `Cargo.toml`.
  - **Tests added (3):** test_xyz_get_tile_fetches_http_body, test_xyz_get_tile_404_returns_tile_not_found, test_xyz_get_tile_5xx_retried (in `tests/xyz_http_test.rs` with in-process TcpListener mock server).

- [x] Compute total-chunks for `ChunkedWriter` instead of writing `0` (completed 2026-05-17)
  - **Done:** `ChunkedWriter` now tracks `chunks_written` (counter), exposes `set_total_chunks(n) -> Self` builder. `finalize()` validates `chunks_written == preset_total_chunks` (if set), then writes a 12-byte footer (`b"CHNK"` + little-endian u64) so readers can discover the definitive count. Returns `StreamingError::IncompleteFinalize { expected, actual }` on mismatch.
  - **Tests added (3):** test_chunked_writer_finalize_patches_total_chunks, test_chunked_writer_with_preset_chunks_matches_on_finalize, test_chunked_writer_preset_mismatch_returns_error.

## Medium Priority
- [x] Connect `cloud::object_store::ObjectUrl` / `ByteRangeRequest` to an actual HTTP client (completed 2026-05-17)
  - **Done:** New `src/cloud/http.rs` — `HttpObjectStore { client: reqwest::Client, retry: RetryPolicy }`. `get_range(url, start, end_inclusive, creds) -> Result<Bytes, CloudError>` issues `Range: bytes=X-Y` HTTP GET; retries on 429/503 via `RetryPolicy`. `get_object(url, creds)` fetches the full body. `head(url, creds) -> Result<ObjectMetadata>` extracts Content-Length, ETag, Last-Modified. `apply_credentials` handles `Anonymous`, `Bearer`, `AccessKey` (X-Access-Key header), `SasToken`; Azure Shared Key and ServiceAccountFile return `UnsupportedCredentials`. Pure-Rust RFC 1123 date parser (`parse_rfc1123_date`). New `CloudError` variants: `HttpError { status, url }`, `TransportError`, `RetryExhausted { attempts }`, `UnsupportedCredentials`, `MissingHeader`. Re-exported from `src/cloud/mod.rs`. Feature-gated under `cloud-http = ["std", "dep:reqwest"]`.
  - **Tests added (7):** test_http_object_store_get_range_returns_slice, test_http_object_store_get_object_full_body, test_http_object_store_head_extracts_metadata, test_http_object_store_429_retried_by_retry_policy, test_http_object_store_anonymous_creds_no_auth_header, test_http_object_store_bearer_creds_auth_header, test_http_object_store_404_returns_http_error. Total: 419 tests pass (with cloud-http).

- [ ] Real `RecordBatch` integration tests for the Arrow IPC path
  - **Goal:** `src/arrow_ipc.rs` (10 KB) has builders but no round-trip test that emits Arrow record batches into a stream and reconsumes them. Add coverage.
  - **Files:** new `tests/arrow_ipc_roundtrip.rs`.
  - **Why deferred:** Pure test-coverage gap, not a functional one.

- [ ] Exactly-once semantics via transactional checkpointing on `CheckpointBarrier`
  - **Goal:** Bring `src/v2/checkpoint.rs` (21.8 KB) from snapshot-only to two-phase commit aligned with sinks (Kafka transactions, Kinesis idempotent writes).
  - **Files:** `src/v2/checkpoint.rs`, `src/state/checkpoint.rs`.
  - **Why deferred:** Depends on sink-side transactional API surface in oxigeo-kafka / oxigeo-kinesis.

- [ ] Watermark propagation across multi-stream joins
  - **Goal:** `src/v2/stream_join.rs` (16.2 KB) computes per-side watermarks but does not propagate combined low-watermark to the merged output operator.
  - **Files:** `src/v2/stream_join.rs`.
  - **Why deferred:** Combined-watermark spec needs `min(left, right)` semantics plus side-stream lateness budget; design fork required.

- [ ] Event-time session windows with gap detection
  - **Goal:** Extend `src/v2/session_window.rs` (14.2 KB; `pending_event_count`, `total_sessions_closed` already exposed at `src/v2/session_window.rs:192-200`) with gap-driven close instead of explicit `close_current_session` only.
  - **Files:** `src/v2/session_window.rs`.
  - **Why deferred:** Needs watermark wiring above.

- [ ] Backpressure propagation source → transform → sink across `v2/backpressure.rs`
  - **Goal:** Currently `src/v2/backpressure.rs:183` notes "Number of items currently staged (not yet drained)" — staging is observable but does not back-signal upstream operators.
  - **Files:** `src/v2/backpressure.rs`, `src/core/stream.rs`.
  - **Why deferred:** Needs operator-graph rewrite (a v0.3.0 item).

- [ ] CDC source for PostGIS via logical-replication slot (pgoutput)
  - **Goal:** Stream-source connector that surfaces row-level changes for spatial tables.
  - **Files:** new `src/sources/postgis_cdc.rs`.
  - **Why deferred:** External dep on oxigeo-services Postgres path.

- [ ] Late-event side-output with configurable allowed-lateness
  - **Goal:** Per Apache Beam / Flink semantics, route late events to a side stream when `event_time + lateness < watermark`.
  - **Files:** `src/windowing/`, `src/v2/`.
  - **Why deferred:** Pair with watermark propagation.

## Low Priority / Future (one-liners)
- [ ] Stream savepoints (snapshot + resume from arbitrary point)
- [ ] Dynamic stream repartitioning without pipeline restart
- [ ] Prometheus / OpenTelemetry metrics exporter (latency + throughput)
- [ ] Stream-to-table lookup join with external cache
- [ ] WASM-based UDF runtime for custom transformations
- [ ] Tiered storage spill (memory → disk → cloud) for unbounded windows
- [ ] Schema evolution support (Avro / Arrow migration in-flight)
- [ ] Stream lineage tracking (per output, contributing input record set)
- [ ] Dead-letter queue for failed transform records

## Cross-crate dependencies
- **Blocks:** oxigeo-services (real-time event push)
- **Blocked by:** oxigeo-kafka (transactional sink), oxigeo-kinesis (cloud source/sink)

## Recently completed (verbatim)
- *(none in this slice)*

---
*Last audited: 2026-05-16*

# TODO: oxigeo-etl

> **Purpose:** Streaming ETL framework for continuous geospatial data processing (source → transform → sink).
> **Status (2026-07-28):** 5,237 LoC · 92 tests all-features / 91 default-features · 0 real-code stubs remaining (the last one, the Kafka source, was retired rather than finished — see below; CRS-transform map and NDVI map both closed — see Recently completed)
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] ~~Wire Kafka source to a real consumer with proper Arc-lifecycle~~ — **cancelled: the
  `kafka` feature was removed in 0.2.1 when `oxigeo-kafka` was retired as a project.**
  `KafkaSource`/`KafkaSourceConfig` and `KafkaSink`/`KafkaSinkConfig` are deleted from
  `src/source.rs` / `src/sink.rs`, along with the `Kafka` variants of
  `SourceError`/`SinkError` and the optional `rdkafka` dependency. `rdkafka-sys` was the
  workspace's only mandatory C-toolchain dependency (cmake → librdkafka), against the
  Pure Rust Policy. This item is closed as won't-do, not as done. See the workspace
  CHANGELOG.md `[0.2.1]` → Removed.

- [x] Implement actual CRS transformation in `MapTransform::transform_crs`
  - **Verified done:** `src/operators/map.rs::transform_crs` now decodes the item as `serde_json::Value`, builds a real `oxigeo_proj::Transformer::from_epsg(source_epsg, target_epsg)`, and applies it. Per the in-source comment, the transformer build is offloaded onto `tokio::task::spawn_blocking` specifically because `oxigeo-proj`'s bundled-PROJ-database open calls `block_on` internally and panics if invoked directly on an async worker thread — this matches a documented workspace gotcha (see project memory: "oxiproj-db `open_bundled()` does `block_on` internally — panics inside any tokio runtime"). No more pass-through/warn-only stub.

- [x] Implement actual NDVI calculation in `MapTransform::calculate_ndvi`
  - **Verified done:** `src/operators/map.rs::calculate_ndvi` decodes the item, extracts `red`/`nir` bands via `extract_band`, validates equal length, and computes the real `(NIR-RED)/(NIR+RED)` vector — no more warn-and-pass-through.

- [x] Add pipeline checkpoint persistence to disk for crash recovery
  - **Verified done:** `src/stream.rs::StateManager` has real `save_checkpoint(&self, pipeline_id)` (`:161`) and `load_checkpoint(&self, pipeline_id)` (`:187`) methods (plus convenience wrappers at `:299,308,315`), and `src/pipeline.rs`'s run loop calls `load_checkpoint` on startup (`:239`) and `save_checkpoint` both periodically during the run (`:322`) and on completion (`:343`).

## Medium Priority
- [ ] Replace per-row PostGIS INSERT with `COPY ... FROM STDIN` bulk load
  - **Goal:** Order-of-magnitude faster `PostGisSink::flush_buffer`; current `src/sink.rs:434-446` does one `INSERT` per item.
  - **Files:** `src/sink.rs:421-449` (existing).
  - **Why deferred:** Functional today; optimisation, not correctness.

- [ ] Implement S3 multipart upload in S3Sink
  - **Goal:** Use `MultipartUpload` for items > `part_size` (5 MiB default at `src/sink.rs:182`); current `put_object` per item is inefficient for large payloads.
  - **Files:** `src/sink.rs:198-262` (existing).
  - **Why deferred:** Functional for small items; deferred until customer with multi-GiB items appears.

- [ ] Add backpressure propagation from sink to source
  - **Goal:** When `BufferedStream` reaches `max_buffer`, signal upstream `Source` to pause via async semaphore.
  - **Files:** `src/stream.rs:66-86` (existing `BufferedStream`).
  - **Why deferred:** Default tokio mpsc channel already provides some backpressure via bounded send; explicit propagation only needed for non-Tokio sources.

- [ ] Add stream-to-stream join operator (`src/operators/join.rs`, 500 LoC stub-free)
  - **Goal:** Hash-join two streams on keyed attribute with configurable join type (inner/left/full).
  - **Files:** `src/operators/join.rs` (existing).
  - **Why deferred:** Operator skeleton exists; full implementation requires window semantics (next item).

- [ ] Windowed aggregation operator (tumbling/sliding/session)
  - **Goal:** Time-based or count-based windows; emit aggregates on window close.
  - **Files:** `src/operators/window.rs` (existing, 451 LoC), `src/operators/aggregate.rs` (existing, 457 LoC).
  - **Why deferred:** Skeleton exists with config types; stream semantics need event-time + watermarks.

- [ ] HTTP webhook receiver mode (`src/source.rs` `HttpSource`)
  - **Goal:** Bind to local port; emit `StreamItem` per inbound POST. Complement to current polling mode.
  - **Files:** `src/source.rs:174-242` (existing, feature `http`).
  - **Why deferred:** Pulls in axum/hyper; large API surface.

## Low Priority / Future (one-liners)
- [ ] GeoParquet source/sink for columnar geospatial ETL (oxigeo-geoparquet integration)
- [ ] STAC bbox/datetime filtering refinements in `StacSource` (currently passes through to API)
- [ ] Schema inference from first N records in source
- [ ] Pipeline DAG visualization (DOT/Mermaid export)
- [ ] Dead letter queue for records that fail transformation
- [ ] Incremental/CDC mode (process only new/changed records)
- [ ] REST API for pipeline management (create, start, stop, status)
- [ ] Cron-based scheduler with persistent job state (cron feature already gated)
- [ ] Pipeline template library (common geospatial ETL patterns)
- [ ] Data lineage tracking (source record → output record mapping; oxigeo-security lineage integration)

## Cross-crate dependencies
- **Blocks:** oxigeo-streaming (may share operator types)
- **Blocked by:** oxigeo-proj (for CRS transform), oxigeo-geoparquet (for GeoParquet source/sink — already complete with predicate pushdown)

## Recently completed (verbatim)
- [x] FileSource line-based + chunked reading — `src/source.rs:88-139` (real tokio::fs implementation, not placeholder)
- [x] FileSink with append/truncate, auto-create parent dirs — `src/sink.rs:132-160` (real, mutex-guarded `tokio::fs::File`)
- [x] STAC catalog source with bbox/collection/datetime/limit query parameters — `src/source.rs:308-360`
- [x] HttpSource with timeout, custom headers, configurable chunk size — `src/source.rs:174-242`
- [x] CustomSource and CustomSink wrappers for user-defined async factories — `src/source.rs:424-459`, `src/sink.rs:476-509`
- [x] Pipeline builder with checkpoint_dir/buffer_size/with_checkpointing fluent API — `src/pipeline.rs:92-102`
- [x] BufferedStream + ParallelProcessor + StateManager + StreamConfig — `src/stream.rs` (486 LoC)
- [x] S3Sink via aws-sdk-s3 (single-part put per item) — `src/sink.rs:188-262`
- [x] PostGisSink with deadpool-postgres pool + batch buffer — `src/sink.rs:387-474`

---
*Last audited: 2026-07-28 (status line refreshed: 70→92/91 tests, LoC 6,675→5,237, date bumped; CRS transform, NDVI calculation, and checkpoint persistence all re-verified against source and flipped to done; the Kafka source/sink were **removed outright in 0.2.1** together with the retired `oxigeo-kafka` crate and the `kafka` feature — the long-open "requires proper consumer lifecycle management" stub is closed as won't-do)*

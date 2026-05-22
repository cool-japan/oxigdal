# TODO: oxigdal-etl

> **Purpose:** Streaming ETL framework for continuous geospatial data processing (source → transform → sink).
> **Status (2026-05-16):** 6,675 LoC · 70 tests · 3 real-code stubs (Kafka source, CRS-transform map, NDVI map)
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Wire Kafka source to a real consumer with proper Arc-lifecycle
  - **Verified gap:** `src/source.rs:411-416` — `Err(SourceError::InvalidConfig("Kafka source requires proper consumer lifecycle management. Use a custom source with Arc-wrapped consumer for production use.".to_string()).into())`
  - **Goal:** `KafkaSource::stream()` returns a real `BoxStream<StreamItem>` backed by `StreamConsumer` from `rdkafka`, with offset commit on item-consumed and consumer-group rebalance handling.
  - **Design:** Spawn a `tokio::task` owning the `StreamConsumer` (cannot move into stream's `unfold` because of pin lifetimes); use a `tokio::sync::mpsc::channel(buffer_size)` as the bridge. On `M::Owned`/`M::Borrowed` extract payload bytes → send. Commit offsets after the sink writes the item (requires checkpoint hook — see streaming-pipeline acknowledgement model). `KafkaSourceConfig.auto_offset_reset` already at `src/source.rs:372-373` wired into `ClientConfig`.
  - **Files:** `src/source.rs:404-422` (rewrite `Source::stream` impl), `src/source.rs` (extend `KafkaSourceConfig` with `enable_auto_commit: bool`).
  - **Tests:** *(proposed)* `test_kafka_source_consumes_messages` (gated `#[ignore]` for embedded broker), `test_kafka_source_respects_offset_reset`, `test_kafka_source_handles_consumer_error`. Feature-gated `kafka`.
  - **Risk:** rdkafka's `StreamConsumer` requires librdkafka C deps (Pure Rust policy: `kafka` is already an opt-in feature gate; document non-default).
  - **Prerequisites:** None — `rdkafka = 0.39` already in `Cargo.toml`.

- [ ] Implement actual CRS transformation in `MapTransform::transform_crs`
  - **Verified gap:** `src/operators/map.rs:192-196` — `tracing::warn!("CRS transformation not yet implemented: {} -> {}", source_epsg, target_epsg);` followed by `Ok(item)` (pass-through).
  - **Goal:** Decode `StreamItem` bytes as GeoJSON geometry, reproject coordinates via `oxigdal-proj`, re-encode bytes. Optional struct-pipeline mode that accepts Arrow geometry binary.
  - **Design:** Detect input format from first byte: `{` → GeoJSON, `\x01`/`\x00` LE/BE → WKB. Build `oxigdal_proj::Transformer::from_epsg(source, target)`, walk coords (Point/LineString/Polygon/Multi*), apply transform, re-serialize. Cache transformer instance per call (closures own one).
  - **Files:** `src/operators/map.rs:184-201` (replace body), add `oxigdal-proj = { workspace = true }` to `Cargo.toml`.
  - **Tests:** *(proposed)* `test_transform_crs_point_geojson`, `test_transform_crs_polygon_wkb`, `test_transform_crs_unsupported_format_errors`, `test_transform_crs_4326_to_3857_at_equator`.
  - **Risk:** Workspace already has `oxigdal-proj` (379 tests, see MEMORY.md). Adding it pulls proj-sys transitively (feature-gated in oxigdal-proj as `system-proj`); ensure default-features=false to stay Pure Rust.
  - **Prerequisites:** None.

- [ ] Implement actual NDVI calculation in `MapTransform::calculate_ndvi`
  - **Verified gap:** `src/operators/map.rs:208-210` — `tracing::warn!("NDVI calculation not yet implemented");` followed by `Ok(item)` (pass-through).
  - **Goal:** Compute `(NIR - RED) / (NIR + RED)` over Arrow-encoded raster bands; emit single-band NDVI raster bytes.
  - **Design:** Accept config `NdviConfig { red_band: usize, nir_band: usize, nodata: Option<f64> }`. Parse Arrow IPC stream from `StreamItem`, slice band columns, vectorize the formula with SIMD-friendly tight loop, propagate NaN through nodata. Output: Arrow IPC of single Float32 column tagged `"ndvi"`.
  - **Files:** `src/operators/map.rs:204-213` (replace body), add `arrow` workspace dep if absent.
  - **Tests:** *(proposed)* `test_ndvi_vegetation_positive`, `test_ndvi_water_negative`, `test_ndvi_nodata_propagates`, `test_ndvi_division_by_zero_handled`.
  - **Risk:** Calling convention — current signature has no config. May need a new `calculate_ndvi_with_config(red, nir)` API; keep zero-arg `calculate_ndvi()` as alias to band-0/band-1 defaults.
  - **Prerequisites:** None.

- [ ] Add pipeline checkpoint persistence to disk for crash recovery
  - **Goal:** `Pipeline::run()` periodically flushes `(pipeline_id, last_committed_offset, item_count)` to `checkpoint_dir/<pipeline_id>.json`; on startup, `load_checkpoint` (already defined in `src/pipeline.rs:227`) restores state.
  - **Design:** `StateManager::save_checkpoint(id, &state)` writes JSON via `tokio::fs::write` with `<id>.tmp` rename for atomicity. State struct: `{pipeline_id, items_processed, last_source_offset: Option<String>, timestamp}`. Pipeline driver calls every N items (configurable in `PipelineConfig`).
  - **Files:** `src/pipeline.rs:217-235` (extend run loop with periodic save), `src/stream.rs` (`StateManager` exists; add `save_checkpoint`/`load_checkpoint` impls).
  - **Tests:** *(proposed)* `test_checkpoint_saved_periodically`, `test_checkpoint_atomic_rename`, `test_checkpoint_resume_from_last_offset`, `test_checkpoint_missing_returns_empty_state`.
  - **Risk:** Concurrent writes if user instantiates multiple `Pipeline`s with same `id` — document or use file-lock (advisory `fcntl`).
  - **Prerequisites:** None — `StateManager` skeleton at `src/stream.rs`.

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
- [ ] GeoParquet source/sink for columnar geospatial ETL (oxigdal-geoparquet integration)
- [ ] STAC bbox/datetime filtering refinements in `StacSource` (currently passes through to API)
- [ ] Schema inference from first N records in source
- [ ] Pipeline DAG visualization (DOT/Mermaid export)
- [ ] Dead letter queue for records that fail transformation
- [ ] Incremental/CDC mode (process only new/changed records)
- [ ] REST API for pipeline management (create, start, stop, status)
- [ ] Cron-based scheduler with persistent job state (cron feature already gated)
- [ ] Pipeline template library (common geospatial ETL patterns)
- [ ] Data lineage tracking (source record → output record mapping; oxigdal-security lineage integration)

## Cross-crate dependencies
- **Blocks:** oxigdal-streaming (may share operator types)
- **Blocked by:** oxigdal-proj (for CRS transform), oxigdal-geoparquet (for GeoParquet source/sink — already complete with predicate pushdown)

## Recently completed (verbatim)
- [x] FileSource line-based + chunked reading — `src/source.rs:88-139` (real tokio::fs implementation, not placeholder)
- [x] FileSink with append/truncate, auto-create parent dirs — `src/sink.rs:132-160` (real, mutex-guarded `tokio::fs::File`)
- [x] STAC catalog source with bbox/collection/datetime/limit query parameters — `src/source.rs:308-360`
- [x] HttpSource with timeout, custom headers, configurable chunk size — `src/source.rs:174-242`
- [x] CustomSource and CustomSink wrappers for user-defined async factories — `src/source.rs:424-459`, `src/sink.rs:476-509`
- [x] Pipeline builder with checkpoint_dir/buffer_size/with_checkpointing fluent API — `src/pipeline.rs:92-102`
- [x] BufferedStream + ParallelProcessor + StateManager + StreamConfig — `src/stream.rs` (486 LoC)
- [x] Kafka sink producer with compression config — `src/sink.rs:290-358` (real, feature `kafka`)
- [x] S3Sink via aws-sdk-s3 (single-part put per item) — `src/sink.rs:188-262`
- [x] PostGisSink with deadpool-postgres pool + batch buffer — `src/sink.rs:387-474`

---
*Last audited: 2026-05-16*

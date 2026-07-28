# TODO: oxigeo-rs3gw

> **Purpose:** rs3gw storage backend for OxiGeo - High-performance cloud storage access (Pure-Rust S3-compatible gateway wrapper; backends: Local/S3/MinIO/GCS/Azure; Zarr store; LRU/ML cache; encryption; dedup).
> **Status (2026-07-28):** 3,256 Rust LoC · 60 tests (all-features; 16 with default-features only) · 0 literal-stub markers — gaps are feature parity with the underlying `rs3gw = 0.2.1` upgrade and missing modules advertised in doc.
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (next slice — verified gaps)
- [ ] Update stale `RS3GW_VERSION` constant to track the upgraded `rs3gw` dependency
  - **Verified gap:** `src/lib.rs:199` — `pub const RS3GW_VERSION: &str = "0.1.0";` while the recent commit `9a9f4b4 Update rs3gw dependency to version 0.2.1` and `Cargo.toml:33` (`rs3gw.workspace = true`) point at 0.2.1.
  - **Goal:** Constant matches actual `rs3gw` version pulled at compile time, so users introspecting `oxigeo_rs3gw::RS3GW_VERSION` get the truth.
  - **Design:** Pull from build script via `built` or `cargo metadata`; or set via `env!` from a `build.rs` that reads `CARGO_PKG_VERSION` of the `rs3gw` dep using `cargo_metadata` crate. Minimal-risk path: hardcode `"0.2.1"` and add a compile-time assertion test against `rs3gw::VERSION` if rs3gw exposes one.
  - **Files:** `src/lib.rs:199` (one-liner update); optional `build.rs` (~40 LoC).
  - **Tests:** (proposed) `test_rs3gw_version_matches_dep` (asserts `RS3GW_VERSION == rs3gw::VERSION` if exposed; else hardcoded string check).
  - **Risk:** None — constant only.
  - **Prerequisites:** None.

- [ ] Add async streaming `DataSource` reader (sequential `Read` / `AsyncRead`) on top of existing range-fetch path
  - **Verified gap:** `src/datasource.rs` defines `Rs3gwDataSource` with `read_range` semantics over `ByteRange`; no streaming `AsyncRead` adapter wraps it. `oxigeo-core::io::DataSource` is the only trait implemented (verified at `datasource.rs:16`).
  - **Goal:** `Rs3gwDataSource::into_async_reader() -> impl AsyncRead` so GeoTIFF / Zarr readers that consume `tokio::io::AsyncRead` work without an in-process pipe.
  - **Design:** Buffered reader pulling fixed-size chunks via `read_range(offset, chunk)` and `poll_read` semantics. Reuse `moka::future::Cache` already wired (`Cargo.toml:53`).
  - **Files:** (new) `src/datasource/async_reader.rs` (~120 LoC); plumbing in `datasource.rs`.
  - **Tests:** (proposed) `test_async_reader_sequential_read_small`, `test_async_reader_chunked_boundary`, `test_async_reader_returns_zero_at_eof`.
  - **Risk:** Lifetime juggling between `DynBackend` and the `AsyncRead` projection; pin in tests.
  - **Prerequisites:** None.

- [ ] Surface `BackendCapabilities` for capability negotiation (range, multipart, conditional)
  - **Verified gap:** `OxigeoBackend` enum in `src/config.rs:18-67` has no `capabilities()` method; callers cannot tell which backend supports range GETs or multipart writes.
  - **Goal:** `fn capabilities(&self) -> BackendCapabilities` returning a bitset/struct (`supports_range`, `supports_multipart`, `supports_conditional`, `supports_versioning`).
  - **Design:** Static-per-variant table. Match exhaustively. `BackendCapabilities` lives in `src/capabilities.rs`.
  - **Files:** (new) `src/capabilities.rs`; `src/config.rs` (`impl OxigeoBackend`).
  - **Tests:** (proposed) `test_local_caps_no_versioning`, `test_s3_caps_full`, `test_gcs_caps_no_multipart_yet`, `test_azure_caps`.
  - **Risk:** Capabilities may drift if rs3gw adds features; keep table central.
  - **Prerequisites:** None.

- [x] Implement ML-cache prefetch path under `ml-cache` feature
  - **Done (verified 2026-07-28):** `src/datasource.rs` implements real background read-ahead: `maybe_spawn_prefetch(next_start, chunk_len, reads_so_far)` (called from the live read path) eagerly fetches the next `prefetch_radius` contiguous same-sized chunks once `ml_training_threshold` reads have been observed on that source, stashing them in the cache. `CogCacheConfig` (`src/features/caching.rs`) documents this explicitly as "real" prefetching. Honesty note carried in both files: despite the `ml_prefetch`/`ml_training_threshold` field names (kept for API compatibility), this is a **deterministic contiguous-access heuristic, not a trained/learned ML model** — no feature vector, no training step.
  - **Original goal (for reference):** On hit at tile `(x, y)`, asynchronously prefetch the configured `prefetch_radius` neighbours into the cache.

- [ ] Bench-coverage parity for the new 0.2.1 rs3gw cache path
  - **Verified gap:** `benches/datasource_benchmarks.rs` (per `Cargo.toml:72-74` `[[bench]]`) needs to be re-checked against rs3gw 0.2.1 — the `Update rs3gw dependency to version 0.2.1` commit may have changed cache hit-ratio assumptions.
  - **Goal:** Confirm benches still compile and update baselines if cache layout shifted.
  - **Design:** Run `cargo bench -p oxigeo-rs3gw datasource_benchmarks` against rs3gw 0.2.1; capture deltas in CHANGELOG.
  - **Files:** `benches/datasource_benchmarks.rs` (audit only); regenerate baseline.
  - **Tests:** None (benches).
  - **Risk:** Bench regressions are not a CI failure; track as advisory.
  - **Prerequisites:** None.

## Medium Priority
- [ ] AES-256-GCM client-side encryption (already in deps under `encryption` feature)
  - **Goal:** `EncryptionConfig::with_key()` is wired in `features/encryption.rs`; actual `encrypt_chunk` / `decrypt_chunk` operations need integration into the read/write path.
  - **Files:** `src/features/encryption.rs`, `src/datasource.rs` (call sites).
  - **Why deferred:** Feature-gated; only matters for HIPAA/SOC2 deployments.

- [ ] Content-defined chunking dedup for Zarr arrays
  - **Goal:** Already-scaffolded `features/dedup.rs` (`ZarrDedupPresets::medium_chunks`); plug Rabin-fingerprint chunker into write path.
  - **Files:** `src/features/dedup.rs`, `src/store.rs` (`zarr` feature).
  - **Why deferred:** Useful in long-running archives only.

- [ ] Presigned-URL generation (delegates to rs3gw)
  - **Goal:** `OxigeoBackend::presigned_get_url(key, ttl) -> Result<String>`; forward to rs3gw API.
  - **Files:** `src/config.rs`.
  - **Why deferred:** Awaits matching rs3gw 0.2.1 surface verification.

- [ ] Object versioning passthrough (S3 versions, GCS generations)
  - **Goal:** `DataSource::read_version(key, version_id)`.
  - **Files:** `src/datasource.rs`.
  - **Why deferred:** Requires per-backend implementation.

- [ ] Bandwidth throttling for metered connections
  - **Goal:** Token-bucket limiter wrapping the read path.
  - **Files:** (new) `src/throttle.rs`.
  - **Why deferred:** Optional optimization.

- [ ] Cluster status / health-check passthrough for MinIO
  - **Goal:** `MinioBackendBuilder::health_check() -> Result<HealthReport>`.
  - **Files:** `src/config.rs`.
  - **Why deferred:** Ops tooling, not core data path.

- [ ] Range coalescing (merge adjacent `ByteRange`s into one HTTP request)
  - **Goal:** `read_ranges(&[ByteRange])` that batches contiguous ranges.
  - **Files:** `src/datasource.rs`.
  - **Why deferred:** Performance optimization for COG access; needs profiling first.

## Low Priority / Future (one-liners)
- [ ] R2 (Cloudflare) explicit preset.
- [ ] Backblaze B2 native backend.
- [ ] Object-lifecycle rules wrapper.
- [ ] Server-side encryption configuration passthrough (SSE-S3 / SSE-KMS).
- [ ] Multi-region replication coordinator.
- [ ] Storage-cost-estimator from access patterns.

## Cross-crate dependencies
- **Blocks:** oxigeo-zarr (S3 chunk reads), oxigeo-geotiff (COG range reads).
- **Blocked by:** rs3gw upstream (0.2.1 already locked); minor surface tweaks may need rs3gw 0.2.2.

## Recently completed (verbatim)
- (None — existing TODO.md had no `[x]` items.)

---
*Last audited: 2026-07-28*

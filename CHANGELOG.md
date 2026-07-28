# Changelog

All notable changes to OxiGeo will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1] - 2026-07-28

Production-hardening campaign (2026-07): a workspace-wide, multi-agent defect
sweep across all 76 crates surfaced **342 confirmed defects**
(47 critical / 84 high / 83 medium / 33 low). **314 were fixed** across 38 crate
lanes (~520 files changed); the remaining 79 were honestly deferred, each left
with a safe typed-error path — a loud `Unsupported*` / `NotImplemented` /
`DecodingError` rather than silent or fabricated data. Quality gates all green:
`cargo fmt --check` clean; `cargo clippy --workspace --all-features --all-targets`
0 warnings; `cargo nextest run --all-features` 17,723 passed / 0 failed /
100 skipped (16,307 passed / 0 failed / 79 skipped on default features); 416
doc tests passing; `cargo deny check` passing. The categorized list of
deferrals carried to v0.3.0 is in TODO.md.

### Fixed

**Format drivers**

- **oxigeo-jpeg2000**: two CRITICAL correctness bugs fixed — multi-tile decode now
  `Psot`-bounds each tile's bitstream and composites it at its real pixel offset
  (previously every tile silently returned tile 0), and the JP2 box parser now
  recurses into `jp2h` so `ihdr`/`colr` in spec-conformant `.jp2` files are read
- **oxigeo-geotiff**: real planar-configuration (`PlanarConfiguration=2`) decoding;
  authoritative EPSG projected/geographic classification; a working JPEG/WebP writer
  path; the silent `GeoKeyDirectory` error and a policy-violating `expect()` removed;
  a `usize`-overflow bug in header-driven allocation fixed
- **oxigeo** (umbrella): fixed GitHub issue #12, "Metadata missing when reading
  geotif" — the lightweight `extract_tiff_info()` peek parser used by `Dataset::open()`
  (distinct from the full `oxigeo-geotiff` driver above) only scanned a GeoTIFF's
  first 8 KiB, so `ModelPixelScaleTag`/`ModelTiepointTag`/`GeoKeyDirectoryTag` values
  stored out-of-line past that offset — routine for striped TIFFs with many strips —
  were silently treated as absent and `crs()`/`geotransform()`/`bounds()` all returned
  `None` even though the tags were present and well-formed; the peek buffer now
  extends up to a bounded 1 MiB when a georeferencing tag's value lands past the
  initial window, a Y-axis sign inversion in the derived `GeoTransform` is fixed
  (`ModelPixelScaleTag`'s Y scale is a positive magnitude per spec but
  `GeoTransform::north_up` expects a negative `pixel_height`), and `bounds()` —
  previously hardcoded to `None` — is now derived from the geotransform and raster
  dimensions; regression test `test_issue_12_far_offset_georeferencing` added
- **oxigeo-drivers/grib**: CRITICAL DRT 5.40 silent-corruption bug fixed — the GRIB2
  decoder now dispatches on the Data Representation Template number, so a
  JPEG2000/PNG/CCSDS payload can never fall through to the simple-packing
  bit-unpacker; DRT 5.40 is wired to a real Pure-Rust JPEG2000 decode via
  `oxigeo-jpeg2000` (new default-on `jpeg2000` feature)
- **oxigeo-shapefile** (vector drivers): the Polygon reader now reconstructs
  multi-part polygons by ESRI ring winding (clockwise = exterior, CCW = hole) with
  containment-based hole assignment, emitting `MultiPolygon` for multiple exteriors —
  a two-island country shapefile round-trips instead of merging its rings
- **oxigeo-drivers/netcdf** & **oxigeo-drivers/hdf5**: NetCDF-4 reader now recurses
  into HDF5 sub-groups (was silently dropping their variables); the HDF5 writer's
  chunking/compression/fill-value hints are no longer silently dropped (real chunked
  write path plus honest errors for shapes oxih5 cannot represent); real object-header
  parsing so `decode_chunk`/filter-pipeline/chunking are no longer dead code
- **oxigeo-drivers/netcdf** & **oxigeo-drivers/hdf5**: attribute decoding now trusts
  the dataspace-declared element count (`count × dtype_size`) and ignores trailing
  bytes, so scalar/small numeric attributes written with padded payloads no longer
  decode as phantom extra elements — this silently disabled CF `_FillValue`/
  `scale_factor` handling for files written by oxih5 0.2.1, whose `FileWriter` padded
  sub-8-byte scalar attribute payloads; the writer regression is now root-fixed
  upstream in oxih5 0.2.2 (this workspace is pinned to it), and the defensive trim
  stays in place as a belt-and-suspenders guard against older files written by 0.2.1
- **oxigeo-drivers/geoparquet**: XYZ/XYM geometry decode ambiguity fixed

**Algorithms & CRS**

- **oxigeo** (umbrella): CRITICAL `Dataset::clip()` bug fixed — clip now records a
  pixel window that every raster read (`read_band`/`bands`/`statistics`/`convert`/
  `read_window`) crops the source file to, so a clipped dataset no longer silently
  reprocesses the full raster
- **oxigeo-algorithms**: real NEON SIMD (with scalar-parity tests) for morphology
  (3×3 erode/dilate) and threshold kernels; a real CSE (let-binding hoisting) + DCE
  (liveness/reachability) pass for the raster-algebra optimizer
- **oxigeo-proj**: PROJ `+proj=hgridshift` / `+proj=vgridshift` pipeline steps now
  actually apply a grid — new `GridRegistry` + `Pipeline::with_hgrid/with_vgrid` and
  evaluators calling the crate's NTv2 grid parser (a sign bug in it was fixed)

**Server & OGC services**

- **oxigeo-server**: the `/tiles/{layer}/{z}/{x}/{y}.{fmt}` XYZ endpoint now renders
  real raster data — reads the intersecting source window, reprojects Web-Mercator
  tiles into the dataset's native CRS (per-pixel inverse warp for non-3857 data),
  applies the layer colormap/RGB style, and masks off-dataset/nodata pixels as
  transparent — replacing a hard-coded checkerboard
- **oxigeo-services**: WPS `buffer`/`clip`/`union` now perform real geometry math via
  `oxigeo-algorithms` and return the computed GeoJSON (previously ignored their
  inputs); CQL2 gained `!=`/`<>`, `IN (...)`, and `IS [NOT] NULL`

**Query engine**

- **oxigeo-query** / **oxigeo-index**: JOIN output now preserves native column types
  instead of stringifying everything; SELECT projection lists are actually applied;
  HAVING is executed (including aggregates referenced only by HAVING); the WHERE
  evaluator gained `BETWEEN`/`IN`/`CASE`/`CAST` with real type coercion

**ML**

- **oxigeo-ml**: model pruning/quantization no longer corrupts ONNX files — a real
  ONNX protobuf walker (`optimization/onnx_weights.rs`) applies genuine tensor
  transforms; `ModelVersion` `Ord` bug fixed
- **oxigeo-ml-foundation**: the crate now compiles and trains — a genuine trainable
  scirs2-neural backend (real forward/backward/optimizer step with explicit gradient
  routing) replaces code that referenced removed `rand` APIs and mismatched types

**Cloud & DB connectors**

- **oxigeo-postgis**: `Transaction::drop` now issues a real implicit `ROLLBACK`
  (was a log-only message that leaked locks) with a double-take guard
- **oxigeo-db-connectors**: MySQL/TimescaleDB SQL-injection surfaces closed via a new
  `crate::sql` identifier-quoting/literal-escaping module plus parameter binding
- **oxigeo-cloud**: CRITICAL rs3gw tokio nested-runtime panic fixed; byte-range reads,
  the prefetch I/O driver, OAuth2/SAS credential refresh (HttpBackend), and STAC fixes
- **oxigeo-cloud-enhanced**: fabricated Azure (Cost/Monitor/ML/Synapse) and GCP (Vertex
  AI/Dataflow/Cost) clients replaced with real, bearer-token-authenticated REST clients
  behind the existing `azure`/`gcp` features — Azure Cost Management queries/forecasts/
  budgets/Advisor, Azure Monitor metrics/Log Analytics/alerts/diagnostic settings, Azure
  ML v2 control-plane compute/model/endpoint/job management, Synapse SQL/Spark pool (ARM)
  management and Spark job/pipeline submission (Livy); GCP Dataflow template launch with
  job status/list/metrics/cancel/drain, Vertex AI model/endpoint/training/batch-prediction
  (long-running-operation polling), and GCP Cost Management via BigQuery billing export
  plus Cloud Billing budgets/Recommender — every previously-fabricated success/ID/
  empty-list is now a real call or an honest typed `NotImplemented`. True data-plane
  operations a control-plane REST client can't mint stay `NotImplemented` (Monitor
  metric/diagnostic ingestion, Cost alert/export, Synapse `execute_query`, ML
  `invoke_endpoint`, GCP cost forecast/export)

**HA & infra**

- **oxigeo-ha**: PITR, snapshot, backup, and DR were entirely fabricated (canned bytes,
  always-pass tests) — replaced with real WAL + on-disk persistence and injectable
  executors; a genuine Raft log-replication module (`failover/log_replication.rs`) with
  `AppendEntries` consistency check, conflict truncation, and majority commit added
- **oxigeo-cluster** (cluster-dist): leader heartbeats now travel over the transport to
  followers (real `AppendEntries`-style RPC + handler) so followers stop perpetually
  re-running elections; W-TinyLFU is now reachable and used by the multi-tier cache
- **oxigeo-kinesis** / **oxigeo-kafka** / **oxigeo-pubsub**: fake/no-op broker paths
  replaced with real implementations and honest errors — Firehose transformation now
  actually happens; Kafka read-process-write exactly-once wired to real transactions

**Bindings**

- **oxigeo-node**: multi-band GeoTIFF save (BIP interleave round-trip); GeoJSON parser
  handles every geometry type; `CancellationToken` wired into batch/parallel processors
  doing real chunked multi-threaded per-pixel work
- **oxigeo-jupyter**: `%crs`/`%bounds`/`%stats` now read a real parsed GeoTIFF dataset
  instead of returning hard-coded `"(example)"` literals
- **oxigeo-python**: `open_raster`/`create_raster` no longer silently discard the
  `driver`/`options` arguments — a real remote/cloud data-source layer (`remote.rs`)
  wires `driver="COG"` and S3/HTTP options through to `oxigeo-cloud`

**no_std & platform**

- **oxigeo-core** / **oxigeo-embedded**: the no_std/embedded claim is now real
  end-to-end — both crates genuinely cross-compile for bare-metal
  `thumbv7em-none-eabihf` (Cortex-M4) and `riscv32imac-unknown-none-elf` (verified with
  actual `--target` builds); `parking_lot`/`crossbeam` are std-gated; `RealtimeScheduler`
  deadline enforcement now actually fires
- **oxigeo-gpu** / **oxigeo-gpu-advanced**: `reproject_gpu`/`execute_gpu` no longer error
  `InvalidBuffer` at runtime — the output buffers now request `MAP_READ` usage
  (verified on Metal)
- **oxigeo-proj**: the `no_std` (`--no-default-features`) build was broken — the crate
  declared `#![cfg_attr(not(feature = "std"), no_std)]` but failed with 63 errors; `extern
  crate alloc` is now unconditional and the alloc-prelude imports
  (`String`/`Vec`/`Box`/`ToString`/`format!`) plus `core::f64::consts` replacements were
  added across the crate, so `no_std` genuinely compiles and its tests pass

**Release-verification pass**

- **oxigeo-cloud**: a doctest in the multi-cloud abstraction example was missing a
  `#[cfg(feature = "s3")]` guard, so `cargo test --doc` failed to compile it under
  default (non-`s3`) features
- **oxigeo-drivers-advanced**: the GeoPackage doctest in `src/lib.rs` had the same bug —
  `gpkg::GeoPackage` used with no `#[cfg(feature = "geopackage")]` guard, because the
  doc prose wrongly called `geopackage` "enabled by default"; fixed with the guard, the
  prose, and a `fn example()`/`async fn example()` in place of `fn main`
- 9 `rustdoc::private_intra_doc_links` violations fixed across 8 files in 7 crates —
  `oxigeo-index`, `oxigeo-gateway`, `oxigeo-security` (×2 files), `oxigeo-drivers/hdf5`,
  `oxigeo-gpu`, `oxigeo-ml-foundation`, `oxigeo-postgis`
- Publish-order bug: `oxigeo-grib` (its default-on `jpeg2000` feature depends on
  `oxigeo-jpeg2000`) was sequenced *before* `oxigeo-jpeg2000` in both
  `~/work/pub_oxigeo.sh` and `scripts/publish-order.txt` — publishing in that order
  would have failed with an unresolved dependency; both are now correctly ordered
- 3 crates were missing `repository` metadata: `oxigeo-geojson-stream`, `oxigeo-index`,
  `oxigeo-noalloc`
- **oxigeo-node**: npm `optionalDependencies` were still pinned to `0.2.0` while the
  package itself is `0.2.1`
- Two hardcoded version strings in HTTP `User-Agent` headers (`oxigeo-stac`,
  `oxigeo-ml`) replaced with `env!("CARGO_PKG_VERSION")` so they can no longer drift
  from the crate version

### Added

- **oxigeo-drivers/zarr**: the empty Zarr v2 reader/writer stubs replaced with a working
  v2 read/write path (chunk-key builder, compressor+filter pipeline, fill values,
  dimension separator, dtype sizing); the ZEP-0002 v3 sharding codec; the fake ZFP codec
  made honest (mode-honoring, overflow-checked)
- **oxigeo-drivers/geoparquet**: the writer now emits real attribute columns and a
  `covering.bbox` column (was silently dropping all attributes); extended-WKB nested
  geometry encoding; Hive-style + spatial (bbox-grid/quadtree/Z-order) partitioning
- **oxigeo-geotiff**: real LERC decode (BitStuffer2 v1/v2/v3) and a JPEG-in-TIFF read
  path that auto-merges shared `JPEGTables` (tag 347)
- **oxigeo-proj**: native forward/inverse projections + round-trip tests for Equidistant
  Conic, Sinusoidal, Mollweide, Robinson, Eckert IV/VI, Cassini-Soldner, and
  Gauss-Krüger (extended zones)
- **oxigeo-drivers/grib**: template-based product-definition expansion (PDT 0.0–0.48
  coverage) and NetCDF CF-conventions v1.11 parsing (`cf_conventions/v1_11.rs`)
- **oxigeo-gpu**: reprojection, raster-algebra, and hillshade WGSL compute shaders;
  multi-GPU workload distribution; WebGPU/WASM shader compilation via a compile-time
  `ShaderRegistry`
- **oxigeo-ml**: ONNX model hot-reload (file-watch + atomic swap), content-addressed
  inference caching (SHA-256 key + LRU), adaptive batch sizing, and model
  versioning / deterministic A/B testing
- **oxigeo** / **oxigeo-streaming**: `DatasetOpenBuilder`/`DatasetCreateBuilder` fluent
  builders; a `FeatureStream`/`TileStream` streaming-iterator API
- **oxigeo-mbtiles** / **oxigeo-gpkg** / **oxigeo-pmtiles**: a real SQLite-backed MBTiles
  writer (now genuinely persists to `.mbtiles`); an opt-in R-tree spatial-index writer
  for GeoPackage
- **fuzz/**: 7 new libFuzzer targets (NetCDF, HDF5 superblock/object-headers, VRT XML,
  GeoJSON, and more), bringing coverage to 11 format/parser targets
- **tests/**: the 1,337-line mock re-implementation in `vector_advanced.rs` replaced —
  33 tests now exercise the real `oxigeo-algorithms` vector stack
- **oxigeo-gateway serving layer**: the previously stubbed `Gateway::serve()` (it accepted
  TCP connections and its `handle_connection` did nothing) is now a real axum 0.8 HTTP
  service — a new `GatewayServer` / `GatewayServerBuilder` wires the crate's
  already-implemented components into a running router:
  - routes: `GET /health`, `GET /gateway/metrics`, `POST /graphql` (plus a GraphiQL page
    when introspection is enabled and a `/graphql/ws` subscription endpoint when
    `enable_subscriptions` is set — that flag is now actually enforced), a `GET /ws`
    WebSocket upgrade (WebSocketManager wiring, default `EchoHandler` route, per-user
    connection caps, ping keepalive, gated on `enable_websocket`), and a load-balanced
    reverse-proxy fallback
  - reverse proxy: a streaming hyper 1 connection client, HTTPS upstreams over the
    Pure-Rust OxiTLS (rustls/RustCrypto) probe connector, hop-by-hop header stripping,
    `FailoverManager` retries that finally honor the previously-ignored
    `LoadBalancerConfig.retry_attempts`, circuit-breaker outcome reporting, and per-attempt
    request timeouts
  - pipeline: query-free trace spans (no query strings), API version negotiation +
    deprecation headers, the in-house middleware chain (CORS with real `OPTIONS` preflight,
    compression, response caching, logging, metrics), JWT/API-key/session auth via
    `MultiAuthenticator` (authenticate-if-present plus a `require_auth` mode, with the
    `require_mfa` flag now enforced), atomic rate limiting with `X-RateLimit-*` /
    `Retry-After` headers, request timeout and body-size limits; a `require_permission`
    RBAC guard is available for route groups and `GatewayError` now implements
    `IntoResponse`
  - honesty fixes: `CachingMiddleware` is now a real LRU+TTL cache instead of a no-op stub;
    compression performs real `Accept-Encoding` negotiation; the 1,865-line
    `middleware::advanced` module (request-ID / enhanced-logging / timeout-header /
    error-handling / histogram-metrics / cache-control) was orphaned — never declared or
    compiled — and is now wired in, compiling and tested; `X-Forwarded-For` is built
    against a trusted-proxy allowlist (`with_trusted_proxies`) rather than blindly trusting
    client-supplied values
  - honest limitations (v0.3.0+): GraphQL resolvers still serve demo/in-memory data (no
    storage backend); middleware-chain hops and proxied requests are buffered (bounded by
    `max_body_size`) while proxy responses stream; there is no WebSocket pass-through
    proxying, no upstream keep-alive pooling, and response-side transformation is not yet
    wired (request-side only)
  - the crate's own test suite grew from 266 to 381 tests (1 → 3 doctests)

### Security

- **oxigeo-services**: WFS-T CQL filtering now **fails closed** on unparseable CQL —
  an unparseable filter previously failed *open*, matching every feature and enabling a
  mass delete/update; it now rejects the request
- **Memory-safety (DoS/OOM hardening)**: header-driven allocation caps added to the
  NetCDF, HDF5, GRIB, and GeoTIFF parsers so a crafted header can no longer trigger a
  multi-gigabyte allocation; includes the GeoTIFF `usize`-overflow fix noted above
- **oxigeo-gateway**: load-balancer health checks now issue genuine HTTP/1.1-over-TCP
  requests (real Pure-Rust TLS via the OxiTLS RustCrypto provider for HTTPS) instead of
  always returning healthy, so a down backend is correctly marked unhealthy; the
  `MalwareScanner` now actually reads and inspects its input; the gRPC health check
  **fails closed** with an honest error rather than reporting unknown backends healthy
- **oxigeo-observability**: health checks do real work (sysinfo disk usage, injectable
  connectivity checker) instead of returning hard-coded `Healthy`; a stub `LabelMatch`
  alert condition that always returned `true` fixed

### Changed

- **oxigeo-db-connectors**: default features made Pure-Rust — the C-FFI database backends
  are now strictly opt-in behind named features
- **oxigeo-query**: `tokio` moved to dev-dependencies and `rayon` gated behind a
  default-on `parallel` feature, so the SQL engine is consumable from
  `wasm32-unknown-unknown`
- **Packaging & legal**: added `NOTICE` and `THIRD_PARTY.md` (Apache-2.0 §4(d)
  attribution + generated third-party license inventory), a committed `deny.toml`
  (advisories + bans + licenses) wired into `cargo deny check`, an in-repo 75-crate
  topological publish-order manifest (previously only in an external script), a license
  note for the vendored `pathfinder_simd`, and `[package.metadata.docs.rs]` fixes on
  the C-FFI-gated crates
- **Supply-chain hygiene**: `.cargo/audit.toml`'s advisory allowlist re-verified against
  the current lockfile and pruned from 21 to 15 entries — `aws-lc-sys`
  (RUSTSEC-2026-0044/-0048) and `tokio-postgres`/`postgres-protocol`
  (RUSTSEC-2026-0178/-0179/-0180) are already patched at our pinned versions, and
  `proc-macro-error2` (RUSTSEC-2026-0173) is no longer in the dependency graph; the new
  `deny.toml` `[bans]` list enforces this workspace-wide, and `tower-http`'s
  `compression-br`/`compression-gzip`/`compression-deflate` features (unused — no
  `CompressionLayer` anywhere — but pulling banned `flate2`/`brotli`/`miniz_oxide` outside
  `deny.toml`'s allowed wrapper scoping) are now explicitly excluded in every consumer;
  `SECURITY.md`'s contact address corrected to `security@cooljapan.tech`
- Dependencies kept current per the Latest Crates Policy (`arrow` 58 → 59, `indicatif`
  0.18 dropping the unmaintained `number_prefix`, `oxih5`/`oxih5-core`/`oxinetcdf`
  0.2.0 → 0.2.2, `scirs2-core` and the `scirs2-{neural,autograd,optimize,datasets,
  metrics,linalg,vision,series}` family 0.6.1 → 0.6.4)
- A further round of Latest Crates Policy bumps: `base64` 0.22 → 0.23, `pollster`
  0.4 → 1.0, `las` 0.9 → 0.10, `jsonwebtoken` 10 → 11, `ed25519-dalek` 2 → 3
  (`std` feature dropped, `zeroize` retained), `azure_core` 1.0 → 1.1,
  `google-cloud-pubsub` 1.1 → 1.2, `statrs` 0.18 → 0.19, `tokio-tungstenite`
  0.29 → 0.30. Only `las` 0.10 required a source change: it replaced the
  per-point `Reader::points()` streaming iterator with a batch/buffer API
  (`Reader::read_all()` / `read_points(n)` returning a `PointData` slab whose
  `.points()` yields the same row-oriented iterator), so `oxigeo-3d`'s
  `LasReader::read_all`/`read_n` were updated accordingly; the other eight
  bumps were drop-in with no source changes required
- **Dependency hygiene**: genuinely-unused dependencies removed from 66 crates'
  `Cargo.toml` files (found via `cargo-machete`, each removal build-verified);
  `deny.toml`'s advisory-ignore list pruned from 15 to 7 entries (the other 8 IDs no
  longer match anything in the current `Cargo.lock`) and its license allowlist trimmed
  of entries no longer reachable in the dependency graph; a `wildcard`-dependency
  `cargo-deny` warning resolved via `allow-wildcard-paths` (three intra-workspace
  dev-dependencies — `oxigeo-3d` → `oxigeo-copc`, `oxigeo-dev-tools` →
  `oxigeo-algorithms`, `oxigeo-qc` → `oxigeo-geojson` — are deliberately unpinned path
  deps so publish ordering doesn't become circular)

### Removed

- **`oxigeo-kafka` is retired as a project, effective 0.2.1.** The crate has been
  deleted from the workspace and **will receive no further releases**; the versions
  already on crates.io (0.0.1 and 0.2.0) have been yanked. This is a deliberate
  retirement, not an oversight — the crate is gone on purpose and is not coming back.

  Removed alongside it: the `kafka` feature of **oxigeo-etl** (and with it
  `KafkaSource`/`KafkaSourceConfig`, `KafkaSink`/`KafkaSinkConfig`, their prelude
  re-exports, and the `Kafka` variants of `SourceError`/`SinkError`), the `kafka`
  feature of **oxigeo-workflow** (which gated an `rdkafka` dependency that no source
  file in that crate ever used), and the `rdkafka` entry in `[workspace.dependencies]`.

  Reason: `oxigeo-kafka` was the **sole mandatory C-toolchain dependency in the entire
  workspace** — `rdkafka-sys` builds librdkafka via `cmake` — which stands against the
  COOLJAPAN Pure Rust Policy. At 4,831 lines it was 0.62% of the workspace's ~778k
  lines of Rust and had **zero reverse dependencies inside the workspace**: nothing
  built on it. As a direct result of the removal, **`cargo check --workspace
  --all-features` no longer requires `cmake` or a C toolchain** and completes clean.

  Migration: use a dedicated Kafka client (e.g. `rdkafka`) directly in your own code,
  or one of the sibling messaging crates that remain supported — `oxigeo-streaming`,
  `oxigeo-kinesis`, `oxigeo-pubsub`, `oxigeo-mqtt`. Workflow definitions can still
  *describe* a Kafka endpoint over the wire: the pure-Rust `IntegrationType::Kafka`
  and `MessageQueueType::Kafka` metadata enums in `oxigeo-workflow` are unchanged.

- **oxigeo-proj**: the `proj-sys` feature and the `proj` C-bindings dependency (C
  bindings to the system libproj) removed, per the COOLJAPAN Pure Rust Policy. All
  coordinate transformation already routed through the pure-Rust `oxiproj` engine, so
  the feature was vestigial — it contributed only an unused error variant and its
  `From<proj::ProjError>` conversion, and no transformation path ever called the C
  library. Its one real effect was that `--all-features` builds required `cmake` and a
  system libproj (the `proj` crate builds PROJ from source), which broke
  `cargo test --workspace --all-features`. For higher-fidelity CRS coverage use the
  pure-Rust `proj-db` feature (oxisql PROJ.db reader, ~7500 EPSG codes) instead.

## [0.2.0] - 2026-07-20

### Changed

- **Project renamed: OxiGDAL → OxiGeo.** Version 0.2.0 is functionally
  identical to 0.1.7 — this is a rename-only release with no feature or
  behavior changes beyond identifiers. The GitHub repository has moved to
  <https://github.com/cool-japan/oxigeo> (old `oxigdal` URLs redirect), and
  v0.1.7 remains the final release published under the OxiGDAL name.

  Migration table (old → new):

  | Area | Old (OxiGDAL) | New (OxiGeo) |
  |------|---------------|--------------|
  | Crates (all 74 published) | `oxigdal`, `oxigdal-<name>` | `oxigeo`, `oxigeo-<name>` |
  | CLI binary | `oxigdal` | `oxigeo` |
  | Environment variables | `OXIGDAL_*` (e.g. `OXIGDAL_CONFIG`, `OXIGDAL_HOST`, `OXIGDAL_PORT`, `OXIGDAL_WORKERS`, `OXIGDAL_LOG_LEVEL`, `OXIGDAL_DATA_DIR`, `OXIGDAL_CACHE_DIR`) | `OXIGEO_*` (`OXIGEO_CONFIG`, `OXIGEO_HOST`, `OXIGEO_PORT`, `OXIGEO_WORKERS`, `OXIGEO_LOG_LEVEL`, `OXIGEO_DATA_DIR`, `OXIGEO_CACHE_DIR`) |
  | Python | PyPI package `oxigdal`; `import oxigdal`; native module `oxigdal._oxigdal` | PyPI package `oxigeo`; `import oxigeo`; native module `oxigeo._oxigeo` |
  | npm | `@cooljapan/oxigdal`; `@cooljapan/oxigdal-node` (+ platform packages); `@cooljapan/oxigdal-geoparquet` | `@cooljapan/oxigeo`; `@cooljapan/oxigeo-node` (+ platform packages); `@cooljapan/oxigeo-geoparquet` |
  | C / mobile FFI | symbol prefix `oxigdal_`; JNI class `com.cooljapan.oxigdal.OxiGDAL`; header `oxigdal_mobile.h`; include guard `OXIGDAL_MOBILE_H` | symbol prefix `oxigeo_`; JNI class `com.cooljapan.oxigeo.OxiGeo`; header `oxigeo_mobile.h`; include guard `OXIGEO_MOBILE_H` |
  | Rust API types | `OxiGdal*` prefixed types (e.g. `OxiGdalError`) | `OxiGeo*` (`OxiGeoError`) |
  | WASM artifacts | `oxigdal_wasm*`; napi artifact `oxigdal.<triple>.node` | `oxigeo_wasm*`; napi artifact `oxigeo.<triple>.node` |
  | Container images | `oxigdal/*`; systemd unit `oxigdal-server.service` | `oxigeo/*`; systemd unit `oxigeo-server.service` |
  | Runtime identifiers | HTTP User-Agent `OxiGDAL/1.0`; Kafka consumer group `oxigdal-etl`; ETL checkpoint dir `oxigdal-checkpoints`; edge cache dir `.oxigdal_cache`; attestation format id `oxigdal-attestation` | HTTP User-Agent `OxiGeo/1.0` (the `oxigeo-stac`/`oxigeo-ml` agents now report `0.2.0`); Kafka consumer group `oxigeo-etl`; ETL checkpoint dir `oxigeo-checkpoints`; edge cache dir `.oxigeo_cache`; attestation format id `oxigeo-attestation` |

- The `oxigdal-*` 0.1.x crates remain published on crates.io for existing
  users; the `oxigeo-*` crates supersede them starting with 0.2.0.

## [0.1.7] - 2026-07-20

### Added

- **oxigdal-cloud-enhanced**: real Azure IMDS managed-identity tokens via `azure_identity::ManagedIdentityCredential`, replacing the placeholder-token stub; real GCP metadata-server access/identity tokens plus IAM Credentials API impersonation, with `GCE_METADATA_HOST` overridable for mock-server tests
- **oxigdal-cloud**: multicloud `build_backend()` factory (S3/GCS/AzureBlob/Http, feature-gated) with a backend cache; `get`/`put`/`delete`/`exists_in_provider` are now functional against real backends
- **oxigdal-drivers-advanced**: JPEG2000 decode now delegates to `oxigdal-jpeg2000` for real decode with full header parsing, replacing the gray-placeholder-pixel stub
- **oxigdal-services**: WFS-T Memory/File transactions fully implemented — insert/update/delete/replace with per-path write serialization
- **oxigdal-services**: WCS File/Url/Memory coverages now do real GeoTIFF read/write via `oxigdal-geotiff`; `encode_as_geotiff` produces real GeoTIFF bytes (was stub output)
- **oxigdal-ml-foundation**: `onnx_export.rs` — pure-Rust ONNX protobuf encoder (ir_version 8, opset 13), round-trip-validated against `oxionnx`
- **oxigdal-ml-foundation**: augmentation noise generation now uses real Gaussian sampling (`scirs2_core` seeded RNG) instead of a synthetic pattern
- **oxigdal-ml**: `OnnxModel::infer_multiband` — real multi-channel `[1, C, H, W]` NCHW tensor inference over a `MultiBandBuffer` (band-sequential channel order, unpacked back into one output band per channel); previously `infer` accepted only a single-band `RasterBuffer`
- **oxigdal-workflow**: Temporal/Prefect `import_workflow` round-trips exporter-generated definitions via metadata headers for lossless ID recovery; export now emits real activity bodies
- **oxigdal-etl**: `calculate_ndvi` map transform implemented, with a zero-denominator guard so masked/no-data pixels emit `0.0` rather than `NaN`
- **oxigdal-cli**: `info`/`stats` implemented for FlatGeobuf, GeoParquet, Zarr, GeoPackage, JPEG2000, COPC, PMTiles, MBTiles (previously "not yet implemented")
- **oxigdal-algorithms**: Lanczos resampling `Wrap` and `Mirror` edge modes implemented (`rem_euclid` / reflect-101)
- **oxigdal-geojson-stream**: TopoJSON writer now emits real arcs for LineString/MultiLineString — open-chain topology with endpoint junctions, no-rotation splitting, and shared-arc dedup via negative reversed indices (was an empty `"arcs": []` stub)
- **oxigdal-gpu**: subgroup/warp operations emit native WGSL subgroup builtins with a workgroup-shared-memory emulation fallback; Metal filter/reduction/nearest-neighbor shader generators implemented; ballot/vote/`SimdGroupOperations` upgraded; new execute-and-compare GPU tests (verified on Metal)
- **oxigdal-bench**: raster/io scenarios now do real work (tile reads, `MmapDataSource`) instead of synthetic placeholders
- **oxigdal-wasm**: `WasmCogViewer.openBytes` — drag-drop local GeoTIFF with full codec support including LZW/Zstd via `CogReader<MemorySource>`; `readTileElevation` (SampleFormat tag 339 parsing); `WasmTerrain` — hillshade/multidirectional hillshade/slope/aspect/color-relief-shaded (Horn method, `ImageData` output); `WasmProjection` + `wgs84ToWebMercator`/`webMercatorToWgs84` shims
- **GeoLab demo** (`demo/cog-viewer`): rebranded OxiGDAL GeoLab — drag-drop loading, terrain-analysis panel, honest byte counters, all CDN dependencies vendored locally; staged to cooljapan.tech/geolab/ (deploy manual)
- **oxigdal-security**: new `attestation` module — tamper-evident session ledger: domain-separated blake3 hash chain (`SessionLog`), Merkle root + per-entry inclusion proofs, Ed25519 session seal (`SessionSigner::seal`), and `verify_attestation()` re-verifying chain/root/signature from the attestation JSON alone; golden-fixture and tamper-detection tests; native skeptic's verifier example `verify_attestation.rs`; compiles for wasm32 under `--no-default-features --features attestation`
- **oxigdal-wasm**: `sentinel` module (GeoSentinel) — `WasmStacClient` Earth Search STAC scene-pair search with client-side cloud/nodata/grid filtering; self-contained UTM↔WGS84 (Krüger series, EPSG 326xx/327xx); `GeoSentinel` change-detection pipeline: windowed COG reads → BOA offset → NDVI drop → fixed/Otsu threshold → polygonization → Karney geodesic hectares → GeoJSON, plus true-color and diff-heatmap RGBA overlays
- **oxigdal-wasm**: `vault` module (GeoVault) — `WasmVaultSession` blake3 hash-chained operation log sealed with Ed25519 into attestation JSON, `verifyAttestation`, blake3 `fileDigestHex` for dropped files
- **oxigdal-wasm**: `anomaly` module — self-contained Z-score / IQR / modified-Z-score / percentile / σ-bounds detectors (parity-ported from `oxigdal-analytics` / `oxigdal-qc`) with mask, `ImageData`, and summary-JSON outputs
- **oxigdal-wasm**: COG reader overview-level reads — full per-overview IFD parsing (each level gets its own tile directory, predictor, and sample layout), `read_tile_level`, and `read_window_u16` / `read_window_rgb8` window assembly; PREDICTOR=2 horizontal-differencing undo (TIFF tag 317) for u8/u16 samples on all tile and window paths
- **oxigdal-geoparquet**: new `plan` / `pushdown` APIs — `plan_pushdown()` computes row-group bbox + attribute-statistics pruning and exact column-chunk byte ranges from metadata alone (zero I/O); `execute_pushdown()` runs pushdown over any `parquet::ChunkReader` (`GeoParquetReader::read_pushdown` is now a thin wrapper)
- **oxigdal-geoparquet**: bbox-column detection now honors GeoParquet 1.1 `covering.bbox` paths from the `geo` metadata (authoritative) with a plain `bbox` struct-root fallback — VIDA-style files (5.9 GB / 9,533 row groups) now prune correctly
- **oxigdal-geoparquet**: `AttributeFilter::Cmp` scalar comparisons (`>`, `>=`, `<`, `<=`, `<>`) with Int64/Float64 literal↔column coercion (a bare integer compares correctly against a Float64 column and a whole-valued decimal against an integer column); multiple filters compose as a conjunction via `with_attribute_filters`
- **oxigdal-wasm-geoparquet** (new crate): browser GeoParquet range-request client — remote footer decode, `SparseChunkReader` over prefetched byte ranges, 64 KiB-gap range coalescing, SQL `WHERE`-fragment → predicate lowering (sqlparser, typed rejections naming unsupported constructs), `RecordBatch` → GeoJSON conversion, and `RemoteGeoParquet` open/plan/query with byte and request accounting (npm: `@cooljapan/oxigdal-geoparquet`)
- **GeoSentinel demo** (`demo/geosentinel`): in-browser Sentinel-2 change detection — STAC pair search, streamed COG windows, NDVI-drop polygons with geodesic hectares, GeoJSON export, before/after crossfade; staged to cooljapan.tech/geosentinel/ (deploy manual)
- **GeoVault demo** (`demo/geovault`): sovereign clean-room workstation — CSP-enforced zero egress, live session ledger, seal → attestation download, independent `verify.html` verifier; synthetic Site K-7 DEM via new `oxigdal-geotiff` example `geovault_scene.rs`; staged to cooljapan.tech/geovault/ (deploy manual)
- **GeoParquet Live demo** (`demo/geoparquet`): bounding-box + SQL attribute queries against the 5.9 GB VIDA GeoParquet via predicate pushdown over HTTP ranges — row-group strip visualization, plan-cost preview before any fetch, Cache API footer caching, offline sample + new `oxigdal-geoparquet` example `generate_sample.rs`; staged to cooljapan.tech/geoparquet/ (deploy manual)
- **oxigdal-server**: new example `render_hero.rs` (DEM → combined hillshade → colormap → PNG)
- docs.rs metadata added to all 64 remaining publishable crates (21 curated for Pure-Rust-only docs builds)
- New `CONTRIBUTING.md` and `CODE_OF_CONDUCT.md`

### Changed

- **oxigdal-cloud-enhanced**: `reqwest` made optional, gated behind the `gcp` feature
- **oxigdal-ml-foundation**: weights save/load moved to `oxicode` (COOLJAPAN no-bincode policy)
- **oxigdal-services**: Database transactions/feature-sources/SQL count moved behind new non-default `postgis` feature (`oxigdal-postgis` pool, `ST_GeomFromGeoJSON`/`ST_AsGeoJSON`); WCS `Url` coverage fetch moved behind new non-default `remote` feature
- **oxigdal-drivers-advanced**: `jpeg2000` feature is now dependency-gated (pulls in `oxigdal-jpeg2000` only when enabled)
- **oxigdal-security**: dependencies split behind new `enterprise` / `tls` / `attestation` features (default enables all three) — the heavyweight server-side surface (tokio, dashmap, petgraph, scirs2-core, oxiarc-zstd, regex, parking_lot, uuid, chrono, crypto stack) is now optional under `enterprise`; `tls` implies `enterprise`; `attestation` pulls only `blake3` + `ed25519-dalek`, keeping the wasm32 surface lean
- **GeoLab demo**: shared `@cooljapan/oxigdal` WASM package rebuilt (pkg refresh) — GeoLab, GeoSentinel, and GeoVault all serve the same refreshed package
- Examples/benches reorganized: 31 orphaned top-level examples wired into `oxigdal-examples` (API rot fixed, 5 duplicates pruned); 11 benches wired into `oxigdal-bench`
- README: stats refreshed, doc links updated, GeoLab hero image made clickable, new `## Demo` section with native-render gallery (`docs/media/`); section grown to `## Demos` with hero/GIF/gallery/honest-notes blocks for GeoSentinel, GeoVault, and GeoParquet Live
- Dependencies bumped to latest per the Latest Crates Policy: `oxiproj`/`oxiproj-core` 0.1.1 → 0.1.2, `oxisql-core`/`oxisql-sqlite-compat` 0.3.2 → 0.4.0, `oxinetcdf` 0.1.4 → 0.2.0, `oxih5` 0.1.4 → 0.2.0 — version-only `Cargo.toml` changes; the `oxih5`/`oxinetcdf` jump to 0.2.0 was verified source-compatible with the `oxigdal-drivers/hdf5`/`oxigdal-netcdf` driver code (no driver-side changes required)

### Fixed (production-hardening campaign, 2026-07)

Parallel multi-lane defect sweep across the workspace: 233 verified defects fixed across
69 crates (correctness, unwrap-elimination, clippy, doc/README accuracy). Headline items:

**Format drivers**

- **oxigdal-geotiff**: floating-point predictor (TIFF `Predictor=3`) decode *and* encode now
  actually implemented — was previously a silent no-op that passed float32/float64 tile data
  through unmodified, corrupting round-trips of predictor-encoded float COGs
- **oxigdal-jpeg2000**: MQ arithmetic decoder `INITDEC` procedure brought into ITU-T T.800
  Annex C spec conformance
- **oxigdal-drivers/gml**: `srsDimension` attribute now parsed, so 3D coordinate geometries
  are no longer silently treated as 2D
- **oxigdal-drivers-advanced (VRT)**: `FirstValid` pixel-function compositing fixed for
  multi-byte sample types (u16/f32/f64 — was only correct for single-byte u8 samples);
  `BandMath` pixel function now substitutes `B10` and higher band variables (previously only
  `B1`–`B9` were recognized, silently dropping bands past 9 from expressions)
- **oxigdal-drivers/hdf5** and **oxigdal-netcdf**: both drivers re-backed by the real
  Pure-Rust `oxih5 0.1.4` / `oxinetcdf 0.1.4` crates (crates.io, no libhdf5/libnetcdf FFI).
  `oxigdal-drivers/hdf5` previously read a custom `OXIGDAL_HDF5_METADATA_V1` JSON sidecar
  and returned zeros for real `.h5` files; it now reads and writes genuine HDF5 via `oxih5`.
  `oxigdal-netcdf` now reads genuine NetCDF-4/CF files via `oxinetcdf`. Public API is
  unchanged (`Hdf5Reader::open`, `Attribute`/`AttributeValue`/`Datatype`/`Hdf5Version`/
  `Hdf5Writer`, `NetCdfReader::open`); 730 tests passing across the 4 affected crates,
  clippy clean. Honest limitations carried forward: `oxih5` 0.1.4 fully reads
  v0-superblock `.h5` files, while v2/v3-superblock files open but currently yield an empty
  tree (best-effort, never faked); the writer produces contiguous real HDF5 (chunk/
  compression hints are dropped, values are correct); the NetCDF reader surfaces the root
  group, and `scale_factor`/`add_offset`/`_FillValue` are exposed as attributes but not
  auto-applied

**Algorithms**

- **oxigdal-algorithms**: the raster/DSL calculator's algebraic optimizer no longer folds
  `x * 0` / `0 * x` to a constant `0.0` — since `NaN * 0.0 == NaN` and `Inf * 0.0 == NaN`,
  the previous simplification silently discarded NoData/Inf semantics in NoData-masked
  raster expressions; covered by a new NaN-semantics regression test
- **oxigdal-algorithms**: Weiler-Atherton polygon clipping's concave-region fallback path no
  longer silently returns a geometrically wrong (angularly-sorted) shape — the mismatch is
  now surfaced as an explicit condition rather than masked as a plausible-looking result;
  full boundary-walk reconstruction for concave fallbacks remains future work (see TODO.md)

**Security**

- **oxigdal-security**: RBAC `resource_pattern` matching is now actually consulted by the
  authorization check — was previously parsed and stored but never read, a
  privilege-widening bug that let any pattern-scoped permission match every resource
- **oxigdal-gateway**: TOTP verification switched to a constant-time comparison and gained a
  ±1 time-step (30s) clock-skew tolerance window per RFC 6238 §5.2; backup-code and
  SMS-challenge comparisons are now constant-time as well

**Cloud & infra**

- **oxigdal-server**: `server.toml` is now actually loaded via `OXIGDAL_CONFIG` in
  Docker/Kubernetes deployments — was previously parsed and then discarded, silently
  running on built-in defaults regardless of the mounted config file
- **oxigdal-stac**: implicit `reqwest` feature pull replaced with an explicit `async`
  feature (with `reqwest` kept as a backwards-compatible alias) — the HTTP client and its
  `aws-lc-sys` transitive dependency are no longer pulled in for consumers who never use the
  async surface
- **oxigdal-streaming**: Kafka/Kinesis connector commit-strategy and consumer-lease
  correctness fixes
- **oxigdal-query**: `GROUP BY` execution implemented in the SQL executor (was previously a
  no-op that ignored the clause)

**Bindings**

- **oxigdal (umbrella)**: `DatasetWriter::finalize()` now writes a real format, or returns a
  typed error, instead of emitting a fake `OXIG`-prefixed placeholder blob on unsupported
  paths

**no_std & platform**

- **oxigdal-core**: now compiles under `--no-default-features --features alloc` (no_std +
  `alloc`, no `std`) — the build previously failed under this combination, blocking
  `oxigdal-embedded`/`oxigdal-noalloc` no_std consumers

### Fixed

- **oxigdal-etl**: `transform_crs` now implemented via `oxigdal_proj::transform_epsg`, offloaded to `tokio::task::spawn_blocking` — previously panicked with "Cannot start a runtime from within a runtime" when invoked inside any Tokio runtime, because `transform_epsg` opens the bundled PROJ database and builds its own current-thread runtime internally; this is a real bug fix, not a hardening change
- **oxigdal-etl**: `calculate_bbox` fixed — was unconditionally returning `[0, 0, 0, 0]`
- **oxigdal-ml-foundation**: unavailable `scirs2` input-gradient paths now return honest typed errors instead of silently returning zero gradients
- **oxigdal-gpkg**: tile matrix set `srs_id` now writes the real EPSG:4326 SRS encoding via new `int2_st()` helper (was a hardcoded placeholder value of `4`)
- **oxigdal-cli**: `merge` placeholder test replaced with a real assertion
- **oxigdal-wasm**: COG IFD parser — `BitsPerSample` / `SampleFormat` entries carrying one SHORT per sample (count > 1, e.g. RGB TCI COGs) were read as inline scalars, yielding a garbage bit depth from the offset word and silently disabling predictor undo for multi-band tiles; arrays now go through offset-following array reads (first entry authoritative)
- **oxigdal-drivers/flatgeobuf**: reader and writer now produce and parse the *real* FlatBuffers wire format — size-prefixed `Header`/`Feature` tables per the official FlatGeobuf schema, written via `flatbuffers::FlatBufferBuilder` and read back through a new bounds-checked vtable walker (`fbs` module) — instead of an ad-hoc custom binary layout; files are now interoperable with GDAL and other FlatGeobuf tooling. New `tests/real_format.rs` independently walks the on-disk bytes to confirm they are genuine FlatBuffers, not just round-trippable against this crate's own reader
- **oxigdal-geotiff**: LERC decode (TIFF Compression tag 34887) now implements the real Esri/GDAL LERC2 bit-stuffed block format — header parsing, run-length-encoded validity mask, `BitStuffer2` variable-bit-width unpacking, and exact dequantization — via a new `lerc_codec::lerc2` decoder; previously the codec only round-tripped its own raw-value payload and returned an explicit error on genuine GDAL/Esri-produced LERC streams. LERC *encoding* to the interoperable bit-stuffed format remains explicitly unimplemented (typed error, not a fabricated blob)
- **oxigdal-jpeg2000**: Tier-2 packet-header parsing (new `tier2::layout`/`tier2::packet`/`tier2::tile` modules) now drives code-block decoding from the real per-(resolution, subband, code-block) precinct geometry and COD progression order, replacing a naive even-division byte split across code-blocks that did not reflect the actual packet structure of real JPEG2000 codestreams. Supports LRCP/RLCP progression, single quality layer, maximum-size precincts, and the reversible 5/3 wavelet; unsupported progression orders or multi-layer streams now return a typed `UnsupportedFeature` error instead of mis-decoding silently
- **oxigdal-drivers/hdf5**: the ScaleOffset (`H5Z_SCALEOFFSET`, id 6) and N-Bit (`H5Z_NBIT`, id 5) filters now implement libhdf5's actual on-disk `cd_values`/per-chunk layouts (matching `H5Zscaleoffset.c`/`H5Znbit.c`) instead of an invented header format, so chunks produced by h5py/netcdf-c decode correctly and chunks written here are byte-compatible with libhdf5; a new `filters::pipeline_message` parser decodes the real Object Header Filter Pipeline message (both v1 and v2 on-disk layouts) that supplies each filter's parameters
- **oxigdal-embedded**: the `power` module now makes explicit that `PowerManager` performs no hardware power/clock transitions unless a board-support `PowerController` is installed (new trait extension point) — CPU-frequency scaling and clock/power gating are SoC-vendor-specific and were previously implied rather than actually performed; `request_mode_strict` added for callers where a silent no-op would be a correctness bug
- **oxigdal-algorithms**: both raster-algebra expression front-ends (the Pest-based `dsl` parser and the hand-written raster calculator parser) are recursive descent and had no bound on input nesting depth — a deeply nested expression such as `((((...))))` or a long `-----x` unary chain aborted the whole process with a stack overflow (`SIGABRT`), an unrecoverable crash reachable from untrusted expression text. Both now enforce a measured `MAX_EXPRESSION_DEPTH` (64) before recursing, returning the typed `AlgorithmError::NestingTooDeep` instead of crashing; wired through to `oxigdal-node`'s error mapping as well
- Test fixtures: two `oxigdal-cli` integration tests silently depended on demo fixtures excluded by `.gitignore` (`demo/cog-viewer/*.zarr`, `*.fgb`), so they only passed on machines where a developer had manually regenerated the fixture locally and failed deterministically on a clean checkout (previously misdiagnosed as a Linux-only flake). `test_read_zarr_info_demo_fixture` is fixed by committing the actual `iron-belt.zarr` fixture; `test_read_flatgeobuf_info_demo_fixture` is fixed by falling back to an equivalent in-process synthesized FlatGeobuf fixture when the demo file is absent, keeping the test self-contained either way
- README: quickstart example now compiles as written (`crs()` returns `Option`)
- Hygiene: removed a stray rustc-ICE dump, auto-fix-generated logs/backups, and 3 stray `.bak` files from crate `src/` trees; `.gitignore` hardened; `.cargo/config.toml` stale `rusqlite`/`proj-sys` entries removed; `pypi-publish.yml` stale `openssl-devel` step removed; `pyproject.toml` and `package.json` synced to 0.1.7

## [0.1.6] - 2026-06-15

### Added

- **oxigdal-shapefile**: Non-UTF-8 DBF encoding support via `encoding_rs` — `resolve_cpg()` maps CPG file labels, `resolve_ldid()` maps LDID byte to IANA encoding, `decode()` transcodes byte slices; `ShapefileReader::open_with_encoding()` and `DbfReader::read_with_encoding()` accept an explicit encoding override (PR #10)
- **oxigdal-proj**: `wkt_to_proj_string()` — converts an OGC WKT-1/WKT-2 CRS string to a PROJ string, enabling `from_wkt` CRS objects to work directly with `Transformer` (PR #9)
- **oxigdal-analytics**: `LocalMoranI::calculate_with_permutations()` — permutation-based significance testing for Local Moran's I spatial autocorrelation (pseudo-p-values under conditional randomisation)
- **oxigdal-cache-advanced**: W-TinyLFU eviction policy — `WTinyLfuEviction<K>` (window + protected/probationary segmented LRU) backed by `CountMinSketch` frequency estimator for O(1) admit decisions
- **oxigdal-copc**: `WaveformPacket` — LiDAR point-format 9 and 10 full-waveform data types (byte-offset, packet-size, return-point-waveform-location, XYZ(t) parametric vector)
- **oxigdal-drivers/hdf5**: HDF5 v2/v3 superblock parser — `SuperblockV2`, `read_superblock_v2()`, `validate_superblock_checksum()` (Jenkins lookup3 hash), enabling full HDF5 V2/V3 file support
- **oxigdal-index**: Delaunay triangulation — `triangulate(points)` (Bowyer-Watson), `Triangulation::convex_hull()` returning vertex indices in CCW order
- **oxigdal-qc**: `BatchRunner` / `BatchReport` / `SeverityCounts` — batch QC over directories; `GpkgValidator` / `GpkgValidationResult` — structural GeoPackage validation; `StacValidator` / `StacValidationResult` — STAC item/collection schema validation; `RadiometricValidator` / `RadiometricValidationResult` / `BandRange` / `SensorProfile` — per-band range validation against sensor profiles (Sentinel-2, Landsat-8/9, custom)
- **oxigdal-sensors**: `MaximumLikelihood` classifier — Gaussian MLC with per-class prior support and `singular_covariance` error variant for degenerate covariance matrices
- **oxigdal-streaming**: `KvStateBackend` — OxiStore-backed persistent state backend for stateful streaming pipelines (replaces in-memory HashMap state)
- **oxigdal-terrain**: GLCM texture derivatives — `glcm_texture()`, `GlcmTextures` (contrast, dissimilarity, homogeneity, energy, correlation, ASM), `GlcmOffset` direction enum; TPI variants — `tpi_annulus()`, `tpi_standardized()`, `landform_classification_tpi()`, parallel editions `tpi_annulus_parallel()` / `tpi_standardized_parallel()`; geomorphons landform classifier — `geomorphons()` (Jasiewicz & Stepinski 2013, 10-class); cost distance / least-cost path — `cost_distance()`, `least_cost_path()`
- **oxigdal-temporal**: Whittaker smoother and Savitzky-Golay filter for time-series gap filling (`WhittakerSmoother`, `SavitzkyGolay`), completing the `gap_filling` module
- **oxigdal-metadata**: DOI/INSPIRE metadata transform support — `transform_doi_locator()`, enabling ISO 19115 locator URIs to be mapped to DOI/INSPIRE-compliant identifiers
- **oxigdal-algorithms**: Viewshed curvature/refraction constants extracted — `EARTH_RADIUS_M` (IUGG 2015, 6 371 000 m) and `REFRACTION_COEFF` (k = 0.13, standard atmosphere) replace magic numbers in viewshed analysis
- **oxigdal (umbrella)**: GPX, KML, and TopoJSON formats now supported in `open()` / vector streaming — detected by file extension and routed to the appropriate parser
- **oxigdal-drivers/geotiff**: `compress_webp_with_params()` — WebP compression with explicit quality/lossless parameters; `image-webp 0.2` added as workspace dep
- **oxigdal-pmtiles**: `MbTilesConn` — OxiSQL-backed MBTiles adapter (`open()`, `open_memory()`, `query_count()`, `query_text()`, `query_blob()`) used internally by PMTiles MBTiles export

### Changed

- **SQLite backend**: `rusqlite` and `libsqlite3-sys` (C FFI) fully eliminated from the entire workspace; all SQLite access now goes through `oxisql-sqlite-compat 0.1.5` (pure-Rust Limbo engine). Affected crates: `oxigdal-db-connectors`, `oxigdal-gpkg`, `oxigdal-drivers-advanced`, `oxigdal-mbtiles`, `oxigdal-pmtiles`
- **oxigdal-security**: TLS stack migrated from `ring`/`webpki-roots` to `oxitls-core` + `oxitls-adapter-rustls-rustcrypto` + `oxitls-webpki-roots` — 100% Pure Rust by default; `tls` feature gating maintained; PBKDF2 key derivation moved from `ring::pbkdf2` to `pbkdf2::pbkdf2_hmac::<sha2::Sha256>`
- **oxigdal-security**: `ring = "0.17"` replaced with `pbkdf2 = "0.13"` in workspace dependencies; `argon2`, `aes-gcm`, `chacha20poly1305` retained as pure-Rust alternatives
- **oxigdal-drivers-advanced**: `rusqlite`/`geopackage` feature made optional (removed from `default` closure); GeoPackage connection now uses `SqliteConnectionBlocking`
- **oxigdal-workflow**: `rdkafka` moved behind `kafka` feature; new `http-client`, `kafka`, `integrations`, and `full` feature flags
- `scirs2-core` / `scirs2-neural` / `scirs2-autograd` / `scirs2-optimize` / `scirs2-datasets` / `scirs2-metrics` / `scirs2-linalg` / `scirs2-vision` / `scirs2-series` updated 0.4.4 → 0.5.0
- `oxionnx` updated 0.1.3 → 0.1.4
- `oxiarc-*` suite updated 0.3.0 → 0.3.3 (archive, core, deflate, lzw, lz4, zstd, bzip2, lzhuf, snappy, brotli)
- `oxicode` updated 0.2.3 → 0.2.4
- Workspace: ~35 inline dependency declarations migrated to `*.workspace = true` (workspace policy compliance)
- `oxigdal-kafka` and `oxigdal-offline` removed from `default-members` (C FFI crates excluded from default workspace builds per Pure Rust Policy)
- `mimalloc` changed to `default-features = false` to avoid C dependency in default build
- Workspace `[patch.crates-io]`: added `oxitls-core`, `oxitls-adapter-rustls-rustcrypto`, `oxitls-webpki-roots` local checkout paths
- **MSRV**: minimum supported Rust version raised 1.85 → 1.89 — the `time 0.3.49` dependency requires Rust ≥1.88; standardized on 1.89 to align with the active oxi-ecosystem cluster

### Fixed

- Pure Rust Policy: `ring`, `rusqlite`/`libsqlite3-sys`, `rdkafka-sys` removed from default feature closure — workspace default build is now 100% C/FFI-free
- `oxigdal-gpkg` change-tracking tests: 11 tests `#[ignore]`ed with explanation comment noting Limbo does not yet fire `AFTER INSERT/UPDATE/DELETE` triggers; remaining test verifies schema creation path

### Security

- Replaced `ring 0.17` (RUSTSEC-2023-advisory dependent) with pure-Rust `pbkdf2 0.13` + existing `argon2`/`aes-gcm`/`chacha20poly1305` alternatives
- `aws-lc-sys`, `rustls-webpki`, `rsa` advisories (RUSTSEC-2026-0044/0048/0049/0097-0099/0104, RUSTSEC-2023-0071) remain in `.cargo/audit.toml` allowlist — all transitive via AWS SDK / rumqttc / azure_core, not directly controllable

## [0.1.5] - 2026-05-22

### Fixed

- **oxigdal-gpu**: WGSL uniform layout in `RayMarchUniforms` — removed stray `_pad1: f32` that shifted every field by 4 bytes and caused the compute kernel to read `max_steps` ≈ 1.05×10⁹, hanging `device.poll(wait_indefinitely)` indefinitely on macOS Metal. The previously-timing-out `test_ray_march_gpu_matches_cpu_when_backend_present` now passes in 0.127s.

## [0.1.4] - 2026-04-19

### Added

- **Wave 1 Algorithms Depth** (`oxigdal-algorithms`): Weiler-Atherton polygon clipping (general polygon-polygon clipping with hole support), Karney's geodesic area formula (sub-meter accuracy on WGS84 ellipsoid), DE-9IM (Dimensionally Extended 9-Intersection Model) topological predicates, marching squares contour extraction for raster isolines
- **Wave 1 ML Migration** (`oxigdal-ml`): Migrated from `ort` to `oxionnx` — Pure Rust ONNX inference runtime aligned with COOLJAPAN Pure Rust Policy; cloud detection, super-resolution, and ONNX model loading now use `oxionnx`
- **Wave 2 R-tree Enhancements** (`oxigdal-index`): Node deletion with tree rebalancing, STR (Sort-Tile-Recursive) bulk loading for O(n log n) construction, k-nearest neighbor search with priority queue, R-tree serialization/deserialization
- **Wave 2 SIMD Resampling** (`oxigdal-algorithms`): AVX2 and NEON intrinsics for bilinear and bicubic resampling kernels; auto-detects CPU features at runtime
- **Wave 2 Raster Polygonization** (`oxigdal-algorithms`): Vector polygon extraction from labeled raster regions with boundary tracing and hole detection
- **Wave 2 Topology-Preserving Simplification** (`oxigdal-algorithms`): Visvalingam-Whyatt and Douglas-Peucker variants that preserve shared boundaries across adjacent polygons
- **Wave 2 NoAlloc Geometry Types** (`oxigdal-noalloc`): `FixedLineString<N>`, `FixedRing<N>`, `BBox3D`, `Mercator` projection helpers, `geohash` neighbour enumeration — all zero-allocation, const-generic capacity
- **Wave 2 PMTiles Reader Completion** (`oxigdal-pmtiles`): Full tile retrieval pipeline with OxiARC decompression (gzip/brotli/zstd), FNV-1a content deduplication on reads, directory navigation for root + leaf directories
- **Wave 2 COPC Reader** (`oxigdal-copc`): Cloud Optimized Point Cloud reader with EPT hierarchy traversal, octree-based spatial queries, and HTTP range request support
- **Wave 2 GeoPackage B-tree + 3D WKB** (`oxigdal-gpkg`): B-tree index support for attribute queries, Well-Known Binary 3D geometry parsing (PointZ, LineStringZ, PolygonZ, etc.)

### Fixed

- **pyo3 0.28 Migration** (`oxigdal-python`): Full migration from pyo3 0.24 to 0.28 — updated `Bound<'py, T>` lifetime parameters, new `IntoPyObject` trait usage, migrated GIL handling APIs
- **Clippy Cleanup** (`oxigdal-drivers/geojson`): Streaming test suite clippy cleanup — removed unused imports, fixed `.collect()` redundancies, corrected error propagation patterns
- **GeoTIFF Metadata Optimizer** (`oxigdal-geotiff`): Improvements to COG metadata optimizer and validator for tile ordering and overview consistency
- **ML Error Types** (`oxigdal-ml`): Refined error taxonomy and `OnnxModel` API for the oxionnx migration

### Changed

- All ONNX inference now routes through `oxionnx` (Pure Rust) — no C++ ONNX Runtime dependency
- Doc examples and subcrate READMEs updated to reference v0.1.4

## [0.1.3] - 2026-03-21

### Fixed
- Fixed all wgpu 29 API breaking changes: `Instance::new` now takes `InstanceDescriptor` by value; `InstanceDescriptor` uses `new_without_display_handle()` instead of `Default::default()`; `bind_group_layouts` now `&[Option<&BindGroupLayout>]` — across all GPU and GPU-advanced crates including benchmarks
- Fixed `libsqlite3-sys` version conflict: downgraded `rusqlite` 0.39→0.37 and `libsqlite3-sys` 0.37→0.35 for `proj-sys` compatibility
- Fixed macOS `librocksdb-sys` dynamic library loading via `.cargo/config.toml` with `DYLD_LIBRARY_PATH`
- Fixed 6 critical bugs in `oxiarc-brotli` (local patch via `[patch.crates-io]`):
  - Encoder `write_window_bits` wrong bit pattern range and encoding
  - Decoder `read_window_bits` incorrect bit-to-lgwin mapping
  - Missing ISEMPTY=0 bit in `encode_meta_block` for non-empty last blocks
  - `BrotliParams::validate()` incorrect lgwin range check
  - `write_code_length_value` values 1 and 5 swapped
  - Huffman decoder EOF/single-symbol edge cases causing "no matching code found" errors
- Fixed `pipeline_builder.rs` clippy: `.map(|l| Some(l))` → `.map(Some)`

### Changed
- All compression/decompression now uses locally-patched `oxiarc-brotli` (via `[patch.crates-io]`)

## [0.1.2] - 2026-03-17

### Added

- **Geometry Validation & Operations** (`oxigdal-index`): `validation.rs` with 7 `ValidationIssue` variants (unclosed ring, self-intersection, hole orientation, etc.), `operations.rs` with centroid, area (Shoelace), perimeter, point-in-polygon (ray casting), Douglas-Peucker simplification, Graham scan convex hull, `is_convex`, `distance`, `ring_bbox`, `buffer_bbox`
- **PMTiles v3 Writer** (`oxigdal-pmtiles`): `PmTilesBuilder` with `add_tile`/`build` API, Hilbert curve tile ID encoding (`hilbert.rs`), LEB128 varint encode/decode (`varint.rs`), content deduplication by FNV-1a hash, PMTiles v3 header/directory serialization
- **Umbrella Crate Integration** (`oxigdal`): 7 new feature-gated re-exports (`gpkg`, `pmtiles`, `mbtiles`, `copc`, `index`, `noalloc`, `services`), `convert.rs` with `DatasetFormat` detection (12 formats), `ConversionPlan`, `can_convert`, `supported_conversions`
- **Subcrate READMEs**: Added README.md for oxigdal-copc, oxigdal-geojson, oxigdal-gpkg, oxigdal-index, oxigdal-mbtiles, oxigdal-noalloc, oxigdal-pmtiles

### Changed

- **Refactored `ogc_features.rs`** (`oxigdal-services`): Split 1,981-line monolithic file into 7 focused modules (`error.rs`, `types.rs`, `query.rs`, `crs.rs`, `server.rs`, `cql.rs`, `mod.rs`) per 2,000-line policy; zero breaking changes
- **Refactored `epsg.rs`** (`oxigdal-proj`): Split 1,873-line file into 5 modules (`types.rs`, `geographic.rs`, `projected.rs`, `utm.rs`, `mod.rs`); zero breaking changes
- **3 new `DatasetFormat` variants** (`oxigdal`): `PMTiles`, `MBTiles`, `Copc` with format detection support
- Workspace now has **76 crates** (~565K total SLoC, ~540K Rust)

### Fixed

- **Clippy `should_implement_trait`** (`oxigdal-netcdf`): Renamed `CfVersion::from_str` → `parse_version` and `CellMethodName::from_str` → `parse_method` to avoid confusion with `std::str::FromStr`

## [0.1.1] - 2026-03-11

### Added

- **EPSG Database Expansion** (`oxigdal-proj`): Expanded from 20 to 211+ EPSG definitions including all 120 WGS84 UTM zones (32601-32660 North, 32701-32760 South), JGD2011, GDA2020, CGCS2000, polar stereographic projections, and State Plane zones
- **JPEG2000 EBCOT Tier-1 Decoder** (`oxigdal-jpeg2000`): Full MQ arithmetic coder with Significance Propagation, Magnitude Refinement, and Cleanup passes; split into submodules (`mq.rs`, `contexts.rs`, `passes.rs`, `decoder.rs`)
- **GeoTIFF Floating-Point Predictor** (`oxigdal-geotiff`): Implemented TIFF Technical Note 3 predictor (horizontal differencing + byte reordering) for Float32/Float64 with full round-trip support
- **Streaming Raster Reader Integration** (`oxigdal-streaming`): Real GeoTIFF driver integration replacing placeholder metadata/data; format detection, metadata from real files, chunk reading via CogReader
- **Pure Rust Compression Migration**: Replaced `flate2` (C) with `oxiarc-deflate` and `zstd` (C) with `oxiarc-zstd` in GeoTIFF driver per COOLJAPAN Pure Rust Policy
- **CLI Command Implementations** (`oxigdal-cli`): Functional `inspect` (reads headers/metadata), `convert` (GeoTIFF-to-COG), and `buildvrt` (generates VRT XML) commands
- **Compression Benchmarks** (`oxigdal-compress`): Real codec benchmarks for deflate, lzw, zstd, bzip2, and lz4 via oxiarc ecosystem
- **Driver Test Coverage**: 20+ integration tests per driver for GeoTIFF, Shapefile, and GeoJSON including round-trip, edge cases, error handling, and multi-band/multi-feature scenarios
- **DEM CLI Terrain Analysis** (`oxigdal-cli`): Activated all 6 terrain operations (`hillshade`, `slope`, `aspect`, `TRI`, `TPI`, `roughness`) — previously blocked by `bail!("not yet implemented")`; slope percent/degree modes and zero-for-flat aspect option added
- **DSL Statistical Functions** (`oxigdal-algorithms`): Implemented `median` (sort-based), `mode` (frequency-map with f64::to_bits), and `percentile` (NumPy-compatible linear interpolation) in DSL function evaluator
- **DSL For-Loop Support** (`oxigdal-algorithms`): `Expr::ForLoop` now evaluates via child scope iteration with 1M-iteration guard against OOM
- **WASM Huffman Decompression** (`oxigdal-wasm`): Implemented full round-trip Huffman decompression — frequency table stored in compressed header, tree reconstructed on decode, single-symbol edge case handled
- **WASM Huffman Decoder** (`oxigdal-wasm`): Canonical Huffman encoding/decoding for WebAssembly compression
- **Server-Side Map Rendering** (`oxigdal-server`): Tile rendering pipeline with dynamic styling
- **Delta Encoding** (`oxigdal-compress`): Delta-of-delta and XOR-delta encoding for time-series raster data
- **Grouped Aggregation Engine** (`oxigdal-analytics`): SQL-style GROUP BY aggregation with min/max/sum/mean/count/variance/stddev
- **HDF5 SWMR Protocol** (`oxigdal-hdf5`): Single Writer Multiple Reader protocol for concurrent HDF5 access
- **FlatGeobuf Spatial Indexing** (`oxigdal-flatgeobuf`): Hilbert R-tree spatial indexing improvements

### Fixed

- **Compilation Blocker**: Fixed workspace version mismatch (0.3.0 → 0.1.1) that blocked all compilation
- **oxiarc-deflate Bug**: Fixed `rle_encode_lengths` Huffman run-length encoding overflow for large homogeneous datasets; applied local patch via `[patch.crates-io]`
- **Dependency Versions**: Corrected oxiarc-* (0.3.0 → 0.2.2), oxicode (0.3.0 → 0.1.1), rs3gw (0.3.0 → 0.1.0), scirs2-core (corrected to 0.3.1)
- **Security**: Updated quinn-proto (RUSTSEC-2026-0037, DoS vulnerability, CVSS 8.7) and yanked wasm-bindgen 0.2.111 → 0.2.114
- **Invalid crates.io Category**: Fixed `science::geo` → `science` in oxigdal crate metadata
- **JPEG2000 Module Conflict**: Removed duplicate `tier1.rs` conflicting with `tier1/` directory module
- **File Size Policy**: Split `reader.rs` (2099 lines) into `reader/mod.rs` + `reader/tests.rs` to comply with 2000-line limit
- **Hardcoded Version Strings**: Replaced hardcoded `"0.1.0"` strings with `env!("CARGO_PKG_VERSION")` in oxigdal-hdf5 and oxigdal-mobile
- **Test Isolation**: Fixed `oxigdal-edge` integration test race condition using unique temp dirs with `AtomicU64` counter
- **ml-foundation Doctest**: Added `#[cfg(not(feature = "ml"))] impl Dataset for GeoTiffDataset` stub to satisfy trait bound in non-ml builds

### Changed

- **Refactored `calculator.rs`** (`oxigdal-algorithms`): Split 1,982-line monolithic file into 7 focused modules (`ast.rs`, `lexer.rs`, `parser.rs`, `optimizer.rs`, `evaluator.rs`, `ops.rs`, `mod.rs`) per 2,000-line policy; zero breaking changes
- **Dependency Updates**: Arrow ecosystem 57→58, sysinfo 0.36→0.38, criterion 0.7→0.8, tokio-tungstenite 0.25→0.28 (API fix applied), redis 0.27→1.0, all SciRS2 subcrates 0.2.0→0.3.1
- Workspace now has **69 crates** (~505K total SLoC, ~480K Rust)
- All internal crates use `version.workspace = true`
- CHANGELOG, README, and publish script updated for v0.1.1

## [0.1.0] - 2026-02-22

**The Independence Release** -- First public release of OxiGDAL, a pure Rust
reimplementation of GDAL for cloud-native geospatial computing.

This release represents the culmination of intensive development across multiple
phases, delivering **~495,961 SLoC** of production-ready Rust code in **68
workspace crates** (474,600 lines of Rust across 1,739 `.rs` files) with **zero
C/C++/Fortran dependencies** in default features. Estimated development cost:
$18.3M equivalent (COCOMO model).

### Added

#### Core Foundation

**Core Library (`oxigdal-core`)**
- Core geospatial data types: `BoundingBox`, `GeoTransform`, `RasterDataType`,
  `RasterBuffer`
- Abstract I/O traits: `AsyncDataSource`, `Dataset`, `RasterDataset`,
  `VectorDataset`
- Storage backends: `LocalFileBackend`, `S3Backend`, `HttpBackend` with HTTP
  range request support
- `RangeCoalescer` for intelligent HTTP request batching and optimization
- Arrow-backed `GeoBuffer` for zero-copy columnar data operations
- Comprehensive error handling with `OxiError` using `thiserror` (no unwrap
  policy enforced workspace-wide)
- `no_std` compatible core types for embedded systems
- Memory-efficient buffer operations with type-safe pixel access

**Algorithms (`oxigdal-algorithms`)**
- SIMD-optimized raster processing: resampling (nearest, bilinear, cubic,
  Lanczos), reprojection, hillshade, slope, aspect, contour generation
- Vector algorithms: topology operations (split, merge, simplify), buffering,
  convex hull, spatial joins, dissolve, and clipping
- Raster algebra DSL powered by a Pest grammar parser
- Portable SIMD with feature-gated AVX2, AVX-512, and ARM NEON paths
- Optional Rayon-based parallelism (`parallel` feature)
- Terrain analysis: aspect (0-360 degrees), slope (degrees or percent),
  curvature (profile and planform), hillshade with configurable azimuth/altitude
- Zonal statistics by polygon zones with support for categorical and continuous
  data
- Douglas-Peucker simplification, positive/negative buffering, boolean
  operations (union, intersection, difference), spatial predicates (intersects,
  contains, within, touches, crosses, overlaps, disjoint)

#### Coordinate Reference Systems (`oxigdal-proj`)

- Pure Rust PROJ reimplementation with zero C dependencies
- 20+ map projections: Transverse Mercator (UTM 1-60), Web Mercator
  (EPSG:3857), Lambert Conformal Conic, Albers Equal Area, Polar
  Stereographic, Azimuthal Equidistant, Oblique Mercator, Japan Plane
  Rectangular (I-XIX zones, JGD2000/JGD2011)
- Complete WKT2 (ISO 19162:2019) parser with WKT1 (OGC 01-009) and ESRI WKT
  backward compatibility
- 211+ embedded EPSG CRS definitions with O(1) lookup
- Datum transformations: 7-parameter Helmert (Bursa-Wolf), 3/5-parameter
  Molodensky, NTv2 grid interpolation, NADCON (NAD27-NAD83)
- Automatic transformation path finding between arbitrary CRS pairs
- SIMD-vectorized batch transforms: < 10ms for 1 million points
- Accuracy within 0.001m of the reference PROJ implementation

#### Geospatial File Format Drivers (11 formats)

- **GeoTIFF / COG** (`oxigdal-geotiff`): Cloud-Optimized GeoTIFF reader/writer
  with tiled access, BigTIFF (> 4GB), overview generation, GeoTIFF 1.1 GeoKey
  directory, compression codecs (DEFLATE, LZW, ZSTD, PackBits, JPEG),
  horizontal differencing predictor, LRU tile cache
- **GeoJSON** (`oxigdal-geojson`): RFC 7946 compliant reader/writer, streaming
  parser for large files, all geometry types, GeoArrow zero-copy conversion,
  configurable coordinate precision
- **Shapefile** (`oxigdal-shapefile`): SHP/SHX/DBF reader/writer with full
  attribute table support and legacy format compatibility
- **FlatGeobuf** (`oxigdal-flatgeobuf`): Packed Hilbert R-tree spatial index,
  streaming feature reads, spatial filtering during decode
- **GeoParquet** (`oxigdal-geoparquet`): WKB and GeoArrow encoding, row group
  statistics with bbox metadata, spatial predicate pushdown, parallel row group
  reading, ZSTD compression (10x faster than GeoPandas for large datasets)
- **Zarr v2/v3** (`oxigdal-zarr`): Array/group hierarchies, zarr.json manifest
  (v3), codec pipeline with compression chain, sharding extension, byte shuffle
  and delta filters, consolidated metadata, parallel chunk loading
- **HDF5** (`oxigdal-hdf5`): Hierarchical data structures, chunking and
  compression, dataset attributes, group navigation
- **NetCDF** (`oxigdal-netcdf`): CF (Climate and Forecast) conventions, unlimited
  dimensions, group hierarchies, variable metadata extraction
- **GRIB** (`oxigdal-grib`): GRIB1/GRIB2 meteorological data, parameter tables,
  level types
- **JPEG2000** (`oxigdal-jpeg2000`): Tier-1 entropy coding, wavelet transforms
  (DWT), codestream parsing
- **VRT** (`oxigdal-vrt`): Virtual raster datasets, on-the-fly processing, band
  mathematics, source mosaicking

**Advanced Drivers** (`oxigdal-drivers-advanced`): Extended format support and
driver plugin architecture

#### Database Connectors (`oxigdal-db-connectors`)

- **PostgreSQL / PostGIS** (`oxigdal-postgis`): Native geometry types
  (WKB/EWKB), GiST/BRIN spatial index integration, bulk COPY protocol,
  connection pooling via `deadpool-postgres`
- **MySQL**: Async connector with spatial type mapping (GEOMETRY, POINT,
  LINESTRING, POLYGON), R-tree spatial index, bulk insert
- **MongoDB**: Document-based geospatial storage with GeoJSON support
- **ClickHouse**: Columnar analytics for geospatial OLAP workloads
- **Cassandra / ScyllaDB**: Wide-column store for time-series geospatial data
- **SQLite / SpatiaLite**: Feature-gated (C dependency, not in defaults per Pure
  Rust Policy), R*-tree spatial index, single-file deployment
- **Redis** (via `oxigdal-gateway`): In-memory caching for tile and query results
- **DuckDB** support via query engine integration

#### Cloud Storage (`oxigdal-cloud`, `oxigdal-cloud-enhanced`)

- **AWS S3**: Full S3 API with range requests for COG byte-range access,
  multipart upload/download
- **Azure Blob Storage**: Azure SDK integration with Data Lake support
- **Google Cloud Storage**: GCS backend with authenticated access
- **RS3GW** (`oxigdal-rs3gw`): S3-compatible gateway adapter (MinIO,
  DigitalOcean Spaces)
- Automatic retry with exponential backoff, client-side caching layer
- Deep cloud integrations: AWS Athena, Glue, Lambda, SageMaker, CloudWatch,
  Cost Explorer; GCP BigQuery, Pub/Sub

#### Streaming and Event Processing

- **Streaming Pipelines** (`oxigdal-streaming`): Real-time data processing with
  backpressure, windowing (tumbling, sliding, session), watermarks for late data
  handling, stateful operators, metrics reporting
- **Apache Kafka** (`oxigdal-kafka`): Producer/consumer for geospatial event
  streams, key-based partitioning, schema registry, exactly-once semantics
- **AWS Kinesis** (`oxigdal-kinesis`): Kinesis Data Streams integration with
  shard parallelism and checkpointing
- **Google Cloud Pub/Sub** (`oxigdal-pubsub`): GCP message queue with
  subscription management and acknowledgment
- **MQTT** (`oxigdal-mqtt`): Lightweight IoT messaging with sensor data types,
  QoS 0/1/2, topic-based routing, retained messages; custom `SensorValue`
  deserializer for robust handling of `serde_json/arbitrary_precision`

#### Query Engine (`oxigdal-query`)

- SQL-like query language for geospatial data with `sqlparser` integration
- Cost-based query optimizer with pluggable rule system
- Optimization rules: Common Subexpression Elimination (CSE), join reordering,
  projection pushdown, predicate pushdown, filter fusion
- Spatial join algorithms: indexed nested loop, spatial hash join
- Arrow-based columnar execution engine

#### Machine Learning and AI

- **ML Runtime** (`oxigdal-ml`): ONNX Runtime 2.0 integration for
  cross-platform inference with multi-backend support (CUDA, ROCm, Vulkan,
  Metal, OpenCL, WebGPU, DirectML), batch preprocessing with automated batch
  size tuning, INT8/FP16 quantization, ResNet/UNet/Transformer/LSTM
  architectures
- **ML Foundation** (`oxigdal-ml-foundation`): Deep learning training
  infrastructure with transfer learning, training loops, Adam/SGD optimizers,
  early stopping, data augmentation, model checkpointing; SciRS2 backend for
  Pure Rust numerical operations

#### GPU Acceleration

- **GPU Core** (`oxigdal-gpu`): WGPU-based GPU computing with Vulkan, Metal,
  DX12, and WebGPU backends; shader compilation for raster operations
- **GPU Advanced** (`oxigdal-gpu-advanced`): Multi-GPU load balancing, memory
  pool management, shader optimization, ML inference pipeline with kernel fusion,
  automatic backend detection
- Optional CUDA backend support

#### Server and API

- **HTTP Server** (`oxigdal-server`): Axum-based REST API for tiles, features,
  and metadata; XYZ tile endpoint; rendering pipeline with on-the-fly processing
- **API Gateway** (`oxigdal-gateway`): Rate limiting (Governor), JWT/OAuth2
  authentication, GraphQL (async-graphql), WebSocket proxying, Redis-backed
  sessions
- **WebSocket** (`oxigdal-ws`, `oxigdal-websocket`): Real-time bidirectional
  protocol for live geospatial data feeds with backpressure handling

#### Enterprise Features

- **Security** (`oxigdal-security`): Encryption at rest (AES-256-GCM,
  ChaCha20-Poly1305), Argon2id password hashing, TLS 1.3 via `rustls`,
  RBAC/ABAC access control, audit logging for compliance (SOC2, GDPR readiness)
- **High Availability** (`oxigdal-ha`): Raft-based consensus, WAL replication,
  automatic failover, health monitoring, circuit breaker pattern
- **Observability** (`oxigdal-observability`): OpenTelemetry tracing and metrics,
  Prometheus exposition, Jaeger backend, structured logging via `tracing`
- **Clustering** (`oxigdal-cluster`): Node management, distributed locking,
  health checks, failure detection
- **Distributed** (`oxigdal-distributed`): Arrow Flight-based data transfer,
  work-stealing scheduler, task graph optimization, fault-tolerant retry

#### ETL and Workflow

- **ETL** (`oxigdal-etl`): Extract-Transform-Load pipelines with
  source/sink abstraction, data validation, incremental processing
- **Workflow** (`oxigdal-workflow`): DAG-based workflow engine (Petgraph),
  cron scheduling, dependency management, state checkpointing

#### Spatial and Domain-Specific

- **3D / Point Cloud** (`oxigdal-3d`): LAS/LAZ point cloud processing, 3D Tiles
  1.0 (B3DM, I3DM, PNTS), glTF export, Delaunay triangulation, terrain mesh
- **Terrain** (`oxigdal-terrain`): DEM processing, hydrological modeling (flow
  direction, flow accumulation), watershed delineation, viewshed analysis,
  terrain ruggedness and topographic position indices
- **Temporal** (`oxigdal-temporal`): Time-series datacube operations, temporal
  aggregation, change detection, gap filling and interpolation
- **Analytics** (`oxigdal-analytics`): Spatial statistics, hot spot analysis
  (Getis-Ord Gi*), clustering, zonal operations, performance profiling
- **STAC** (`oxigdal-stac`): SpatioTemporal Asset Catalog 1.0.0 client,
  catalog/collection/item API, spatial/temporal search
- **Metadata** (`oxigdal-metadata`): ISO 19115:2014, ISO 19139 XML, FGDC CSDGM,
  metadata extraction and transformation between standards
- **Sensors** (`oxigdal-sensors`): IoT sensor observation types, calibration,
  data ingestion
- **Quality Control** (`oxigdal-qc`): Data validation, anomaly detection,
  quality score calculation

#### Platform and Language Bindings

- **WASM** (`oxigdal-wasm`): WebAssembly target with IndexedDB storage, Web
  Worker support, `WasmCogViewer` JavaScript/TypeScript API, Canvas `ImageData`
  integration, bundle size < 1MB gzipped
- **PWA** (`oxigdal-pwa`): Progressive Web App with offline-first architecture,
  Service Worker caching, installable web apps
- **Offline** (`oxigdal-offline`): Offline data sync with conflict resolution,
  operation queue, delta sync
- **Node.js** (`oxigdal-node`): N-API bindings via `napi-rs` for Node.js 16+,
  async Promise-based API, CommonJS and ESM
- **Python** (`oxigdal-python`): PyO3/Maturin bindings, `oxigdal.open()`
  universal opener, `read_geoparquet()` / `read_geotiff()` / `read_zarr()`,
  NumPy array returns, CRS class, algorithm bindings, manylinux2014/macOS/Windows
  wheels
- **Jupyter** (`oxigdal-jupyter`): `evcxr` kernel integration with `plotters`
  visualization, rich display for rasters and vectors
- **Mobile** (`oxigdal-mobile`, `oxigdal-mobile-enhanced`): iOS (Swift FFI) and
  Android (Kotlin/JNI), background processing, battery/network-aware scheduling
- **Embedded** (`oxigdal-embedded`): `no_std` support with `heapless` and
  `embedded-hal`
- **Edge** (`oxigdal-edge`): Edge computing platform with minimal footprint,
  offline-first local database cache, streaming sensor ingestion

#### Developer Tooling

- **CLI** (`oxigdal-cli`): `oxigdal info`, `convert`, `dem`, `rasterize`,
  `warp` commands via Clap
- **Dev Tools** (`oxigdal-dev-tools`): File watching (notify), progress bars
  (indicatif), diff utilities, pretty tables (comfy-table)
- **Benchmarks** (`oxigdal-bench`, `benchmarks/`): Criterion-based benchmarks
  with flamegraph profiling (pprof)
- **Examples** (`oxigdal-examples`): Runnable examples for COG tile serving,
  GeoParquet creation, format conversion, satellite processing

#### Additional Subsystems

- **Compression** (`oxigdal-compress`): Pure Rust compression via OxiArc
  ecosystem (Deflate, LZ4, Zstd, BZip2, LZW, LZH); legacy codec support
  (flate2 rust_backend, zstd, brotli, snappy)
- **Data Synchronization** (`oxigdal-sync`): CRDT-based sync (OR-Set), Merkle
  tree verification, vector clocks for causality tracking, offline queue
- **Caching** (`oxigdal-cache-advanced`): Multi-tier caching (in-memory LRU,
  on-disk, distributed Redis), cache warming strategies
- **Services** (`oxigdal-services`): WMS 1.3.0, WFS 2.0.0, health check
  endpoints

#### Demo Applications

- **COG Viewer** (`demo/cog-viewer/`): Browser-based Cloud-Optimized GeoTIFF
  viewer with JavaScript frontend, MapLibre GL and Leaflet integration

### Changed

- Edition set to Rust 2024 (`edition = "2024"`) with minimum supported Rust
  version 1.85
- Workspace-wide lint configuration: `clippy::unwrap_used = "deny"`,
  `clippy::panic = "deny"` enforced across all 68 crates
- All compression defaults use Pure Rust backends (COOLJAPAN Policy); C-based
  compression libraries are feature-gated or being phased out
- `oxicode` replaces `bincode` for binary serialization (COOLJAPAN Policy)
- `OxiArc` ecosystem (`oxiarc-*`) replaces the `zip` crate for archive
  handling (COOLJAPAN Pure Rust Policy)
- Arrow ecosystem pinned to version 57 across all crates for consistency (upgraded to 58 in v0.1.1)
- Release profile configured with LTO, single codegen unit, and `opt-level = 3`
- `SensorValue` deserialization rewritten with custom `Deserialize` impl to
  handle `serde_json/arbitrary_precision` correctly (replaced derived
  `#[serde(untagged)]` deserialization)
- Edge binary database cache updated for latest schema

### Fixed

- Eliminated 1,143 out of 1,145 `unwrap()` calls across the entire codebase
  (99.83% reduction); remaining 2 are in non-compiled doc comments
- Resolved all 16 rustdoc warnings (feature-gated module links, HTML tags in
  doc comments)
- Fixed `SensorValue` enum deserialization ordering for correct serde roundtrip
  under `arbitrary_precision`
- Fixed all Clippy warnings to achieve zero actionable warnings
- All files refactored to stay under 2,000 lines (maximum observed: 1,976)
- Resolved compilation errors in calculator and buffer modules
- Cleared stale build cache artifacts causing phantom compilation errors
- Fixed Pub/Sub error types and integration test reliability
- Fixed query optimizer rules (CSE, join reordering, projection pushdown)
- Fixed WebSocket protocol handling
- Fixed streaming metrics reporter

### Security

- Encryption at rest via AES-256-GCM and ChaCha20-Poly1305
- Password hashing with Argon2id
- TLS 1.3 transport via `rustls` (no OpenSSL dependency)
- JWT and OAuth2 authentication in the API gateway
- Role-Based Access Control (RBAC) and Attribute-Based Access Control (ABAC)
- Audit logging for compliance (SOC2, GDPR readiness)
- HMAC-SHA256 message authentication for inter-service communication
- All cryptographic operations use pure Rust crates (`ring`, `rustls`,
  `aes-gcm`, `chacha20poly1305`, `argon2`)
- Minimal unsafe code (< 1% of codebase), fully audited and documented
- Vulnerability scanning integrated via `cargo-audit`

### Performance

**Benchmarks Achieved**
- COG tile access: < 10ms (local SSD), < 100ms (cloud S3/GCS)
- Metadata reading: < 5ms for typical GeoTIFF headers
- GeoParquet reading: 10x faster than GeoPandas for large datasets
- PROJ transformations: < 10ms for 1 million points (WGS84 to UTM)
- Docker image size: < 50MB (vs 1GB+ with traditional GDAL)
- WASM bundle: < 1MB gzipped (vs impossible with C-based GDAL)

### Technical Details

**Statistics**
- **Total SLoC**: 495,961 (2,042 files)
- **Rust Code**: 474,600 lines across 1,739 `.rs` files
- **Workspace Crates**: 68
- **Format Drivers**: 11 (GeoTIFF, COG, GeoJSON, GeoParquet, Zarr, FlatGeobuf,
  Shapefile, NetCDF, HDF5, GRIB, JPEG2000, VRT)
- **Map Projections**: 20+ implemented, 211+ EPSG codes embedded
- **Estimated Cost**: $18,275,174 (COCOMO model)

**Platform Support**
- **Operating Systems**: Linux (x86_64, aarch64), macOS (x86_64, aarch64/M1+),
  Windows (x86_64)
- **WebAssembly**: `wasm32-unknown-unknown` target
- **Mobile**: iOS (arm64, simulator), Android (arm64-v8a, armeabi-v7a, x86_64)
- **Embedded**: `no_std` support for microcontrollers

**COOLJAPAN Ecosystem Compliance**
- **Pure Rust Policy**: 100% Rust in default features (C/Fortran feature-gated)
- **No Unwrap Policy**: Zero `unwrap()` in production code (`clippy::unwrap_used
  = "deny"`)
- **Workspace Policy**: All dependencies use workspace inheritance
- **Latest Crates Policy**: All dependencies at latest available versions
- **COOLJAPAN Integration**: SciRS2-Core, OxiCode (not bincode), OxiArc (not
  zip), OxiFFT (not rustfft), OxiZ (not Z3)

### Known Issues

- JPEG2000 support is basic (tier-1 only, no tier-2 optimizations yet)
- Some transitive dependencies have unmaintained advisories (tracked):
  `rustls-pemfile` (RUSTSEC-2025-0134), `sled` (RUSTSEC-2025-0057 fxhash),
  `evcxr` (json 0.12.4), `indicatif` (number_prefix 0.4.0)
- Embedded platforms require nightly Rust for some features

### Migration from GDAL

See [MIGRATION.md](docs/MIGRATION.md) for detailed migration guide from GDAL
C/C++, Rasterio, GeoPandas, and PROJ.

### Roadmap

- **v0.2.0** (Q2 2026): Additional projections (100+ total), GPU acceleration
  expansion, ML pipeline enhancements
- **v0.3.0** (Q3 2026): Real-time streaming improvements, enhanced JPEG2000,
  cloud-native tile server
- **v1.0.0** (Q4 2026): Production stability, LTS commitment, enterprise
  compliance certifications

### Contributors

**Development Team**: COOLJAPAN OU (Team Kitasan)

### Acknowledgments

- **GDAL Project**: Original inspiration and reference implementation
- **GeoRust Community**: Ecosystem collaboration and shared crates
- **PROJ**: Coordinate transformation reference and test suite
- **Rust Community**: Language, tooling, and ecosystem support
- **Specifications**: GeoTIFF, COG, OGC (WMS/WFS), STAC, ISO 19115, RFC 7946
- **Testing Data**: USGS Earth Explorer, Copernicus, OpenStreetMap

---

## Links

- **Homepage**: <https://github.com/cool-japan/oxigeo>
- **Documentation**: <https://docs.rs/oxigeo>
- **Issue Tracker**: <https://github.com/cool-japan/oxigeo/issues>

[Unreleased]: https://github.com/cool-japan/oxigeo/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/cool-japan/oxigeo/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/cool-japan/oxigeo/compare/v0.1.7...v0.2.0
[0.1.7]: https://github.com/cool-japan/oxigdal/releases/tag/v0.1.7
[0.1.6]: https://github.com/cool-japan/oxigdal/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/cool-japan/oxigdal/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/cool-japan/oxigdal/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/cool-japan/oxigdal/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/cool-japan/oxigdal/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/cool-japan/oxigdal/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/cool-japan/oxigdal/releases/tag/v0.1.0

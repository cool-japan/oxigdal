# OxiGeo TODO

> Version: 0.2.4 (2026-08-18) | previous release: 0.2.3 (2026-08-05) | 75 crates | 18,327 tests passing (101 skipped), 0 failures (`--all-features`); 16,862 passing (80 skipped) on default features | 414 doc tests passing (86 ignored) | ~814K Rust SLoC | clippy clean (`--all-features --all-targets`) | `cargo deny check` passing

---

## v0.2.4 — Correctness Release, Release-Ready 2026-08-18 (branch: 0.2.4)

[GitHub issue #17](https://github.com/cool-japan/oxigeo/issues/17): `GeoPackage::from_bytes`/`load_contents()` rejected any `sqlite_master` row wide enough to spill onto SQLite overflow pages (e.g. a ~5000-character QGIS layer name) — the local-payload split used the wrong formula and the overflow-page chain was never followed. [Issue #18](https://github.com/cool-japan/oxigeo/issues/18): `oxigeo-vrt` rejected every GDAL-written `SrcRect`/`DstRect` carrying the sub-pixel values `gdalbuildvrt`/`gdalwarp -of VRT` actually write (`str::parse::<u64>` on a value like `9783.50000000003`) — now parsed as `f64`. [Issue #19](https://github.com/cool-japan/oxigeo/issues/19): VRT mosaic compositing let the first source to cover a pixel win even when all it had there was nodata, punching holes along overlap bands — compositing now honours each source's `<NODATA>` in document order like GDAL. Beyond the three issues: the browser COG URL path (`AdvancedCogViewer`) works end to end for the first time (new `buffered_source` sync-over-fetch driver), GDAL internal-mask IFDs are now excluded from overview-level counting consistently across `oxigeo-geotiff`/`oxigeo-wasm`, `oxigeo-proj` gained a verified `no_std`/`proj-db`/`proj4rs-compat` feature matrix and no longer changes transform results depending on the `proj-db` feature, the SIMD batch fast path is now gated to configurations it can reproduce faithfully (previously silently mis-projected some CRS pairs), and the embedded EPSG registry gained two large correctness passes (US State Plane native units, JGD2011 Japan Plane Rectangular zones I–XIX). Gates green: `cargo fmt --check` clean, `cargo clippy --workspace --all-features --all-targets` 0 warnings, `cargo nextest run --all-features` 18,327 passed / 0 failed / 101 skipped (16,862 / 0 / 80 skipped on default features), 414 doc tests passing (86 ignored), `cargo deny check bans` passing, `cargo publish --dry-run` clean on the leaf crate (`oxigeo-core`). Full categorized list in CHANGELOG.md `[0.2.4]`.

### Fixed in 0.2.4
- [x] **`oxigeo-gpkg`**: overflow-page B-tree cells read correctly (issue #17) — local-payload size computed against the true usable page size, overflow chain followed
- [x] **`oxigeo-vrt`**: `SrcRect`/`DstRect` parse GDAL's fractional numeric formats (issue #18); mosaic compositing honours per-source `<NODATA>` in document order (issue #19)
- [x] **`oxigeo-proj`**: embedded EPSG registry corrected — NAD83/State Plane native units (US survey/international feet, ~79 entries) and JGD2011 Japan Plane Rectangular zones I–XIX (previously misregistered as UTM or absent); SIMD batch fast path gated against datum/ellipsoid/unit mismatches it can't reproduce; `proj-db` feature no longer changes transform results; `no_std`/`proj4rs-compat` builds compile for the first time
- [x] **`oxigeo-wasm`/`oxigeo-geotiff`**: GDAL internal-mask IFDs excluded from overview-level counting/indexing everywhere; browser `AdvancedCogViewer.open()` works for the first time (synchronous-over-`fetch` buffered range source)
- [x] **`oxigeo-geoparquet`**: encoding-aware geometry decode (GeoArrow no longer misread as WKB); null geometries stay index-aligned with property rows

### Added in 0.2.4
- [x] `oxigeo-geoparquet`: `GeoParquetReader::from_bytes`, `read_geometries_optional`/`extract_geometries_optional`, `geometry_encoding()`
- [x] `oxigeo-geotiff`: `tiff::is_mask_ifd`, `CogReader::{ifd_count, level_ifd, level_ifd_index, tile_pixel_size}`
- [x] `oxigeo-gpkg`: `GeoPackage::scan_table_by_name_typed` (SQLite REAL-affinity-aware scan)

## v0.2.3 — Warped VRT + Vector Layers Complete, Release-Ready 2026-08-05 (branch: 0.2.3)

[GitHub issue #15](https://github.com/cool-japan/oxigeo/issues/15): `oxigeo-vrt` rejected
every `gdalwarp -of VRT` product ("Band must have at least one source or a pixel
function") because the driver had no concept of a Warped VRT's `<GDALWarpOptions>` block.
Now implemented for real: new `warp.rs`/`warped.rs`/`srs.rs`/`source_dataset.rs` (1,635
lines) give `oxigeo-vrt` a backward warp engine, depth-aware WKT `AUTHORITY` resolution
(fixed a WKT naming EPSG:4326 that silently resolved to the spheroid's EPSG:7030),
`relativeToVRT` round-tripping, and a quick-xml 0.41 entity-reference fix. [Issue
#16](https://github.com/cool-japan/oxigeo/issues/16): vector-layer support was
incomplete — `Dataset::open` on a GeoPackage always reported 0 layers, and there was no
public API to read a layer's features. Now implemented: new `layer.rs`/`gpkg_schema.rs`
(1,405 lines) give `oxigeo::Dataset` a `layers()`/`layer()`/`layer_by_name()`/
`Layer::features()` API for GeoPackage/Shapefile/GeoJSON, plus a GeoPackage `fid`
rowid-alias fix and a table-constraint-parsed-as-column fix. [Issue
#14](https://github.com/cool-japan/oxigeo/issues/14) needed no code change — the readers
it asked for shipped in 0.2.2. Gates green: `cargo fmt --check` clean, `cargo clippy
--workspace --all-features --all-targets` 0 warnings, `cargo nextest run --all-features`
18,184 passed / 0 failed / 101 skipped (16,722 / 0 / 80 skipped on default features), 412
doc tests passing (86 ignored), `cargo deny check bans` passing, `cargo publish --dry-run` clean on the
leaf crate (`oxigeo-core`; dependent crates cannot dry-run until 0.2.3 is actually
published — registry resolution, not a defect). Full categorized list in CHANGELOG.md
`[0.2.3]`.

### Fixed in 0.2.3
- [x] **`oxigeo-vrt`**: Warped VRTs no longer rejected at parse time
  (`VrtDataset::is_warped` relaxes the source/pixel-function rule when a validated
  `<GDALWarpOptions>` block is present); depth-aware `AUTHORITY`/`ID` WKT resolution
  (`srs::resolve_crs`, bracket-depth tracked so the *root* CRS code is read, not the
  first — usually the spheroid's — code in the string); `relativeToVRT` round-trip on
  both parse and write; quick-xml 0.41 `Event::GeneralRef` entity events (`&quot;`/
  `&amp;`/`&#34;`) no longer dropped, which had corrupted escaped WKT/paths;
  `VrtReader::read_window`'s unchecked `band - 1` underflow replaced with `checked_sub`
  + typed error
- [x] **`oxigeo` facade**: `Dataset::open(".vrt")` no longer returns a zero-filled
  `DatasetInfo` (new `extract_vrt_info` header probe), and every raster read method
  (`read_band`/`read_window`/`read_interleaved`/`_into` forms) now dispatches to the VRT
  reader — including through nested warps and mosaics — instead of being hardwired to
  GeoTIFF only
- [x] **`oxigeo` facade / GeoPackage**: `Dataset::open("x.gpkg")` always reported 0
  layers (`open_vector` had no GeoPackage arm) — fixed via new `extract_gpkg_info`;
  GeoPackage `fid` read back `NULL` on every feature (SQLite `INTEGER PRIMARY KEY`
  rowid-alias not substituted) — fixed in `gpkg_schema`; named table-level constraints
  (`CONSTRAINT ... PRIMARY KEY (...)`) were parsed as bogus extra columns — fixed via
  `is_table_constraint`

### Added in 0.2.3
- [x] `oxigeo-vrt`: `warp` module (`WarpOptions`, `WarpResampleAlg`, `WarpKernel`,
  `WarpBandMapping`, `InitDest`, `ReprojectionTransformer`, `GenImgProjTransformer`),
  `srs::resolve_crs`, `source_dataset::SourceDataset` (nested-VRT recursion,
  `MAX_VRT_NESTING = 16`), `VrtError::EmptyWindow`
- [x] `oxigeo::Dataset::{layers, layer, layer_by_name, layer_names}`, `Layer::features()`;
  new `oxigeo::{Layer, LayerFeatures}` and re-exported `oxigeo::{Feature, FieldValue,
  Geometry}`

### Known limitations carried into 0.3.0
- [ ] `oxigeo-vrt` warp engine resamples Cubic/CubicSpline/Lanczos/Average/Mode
  bilinearly rather than with their named kernel (`WarpResampleAlg::is_kernel_exact()`
  reports which algorithms are exact today: `NearestNeighbour`/`Bilinear` only)
- [ ] `Dataset::layers()` covers GeoPackage (feature `gpkg`)/Shapefile/GeoJSON only;
  FlatGeobuf and GeoParquet return `OxiGeoError::NotSupported` and remain reachable only
  through the streaming feature API

## v0.2.2 — Issue #14 Fix Campaign Complete, Release-Ready 2026-07-30 (branch: 0.2.2)

[GitHub issue #14](https://github.com/cool-japan/oxigeo/issues/14): `Dataset::read_band`
silently ignored its `band` argument on multi-band rasters, returning the whole
pixel-interleaved image. Root-caused to the GeoTIFF driver's block-decode engine
(rewritten as `band_read.rs`/`band_read/multi.rs`), which also surfaced the identical
defect pattern — wrong planar-config assumption or wrong byte order on multi-band raster
reads — independently re-implemented in a dozen other crates (QC, server, mobile, WASM,
WCS, Node, CLI, ML, Jupyter, VRT), plus several unrelated bugs found along the way. 192
files changed; 33 new `issue_14_*`-named files (30 regression tests, 2 benchmarks, 1
example). Gates green: `cargo fmt --check` clean, `cargo clippy --workspace
--all-features --all-targets` 0 warnings, `cargo nextest run --all-features` 18,133
passed / 0 failed / 101 skipped (16,684 / 0 / 80 skipped on default features), 402 doc
tests passing, `cargo deny check` passing, `cargo publish --dry-run --workspace` clean
across all 75 crates. Full categorized list in CHANGELOG.md `[0.2.2]`.

### Fixed in 0.2.2
- [x] **`oxigeo-drivers/geotiff`**: `read_band` root-cause fixed (new `band_read`/
  `band_read::multi` engine); predictor cross-band-stride bug on planar files;
  LERC+predictor undefined-combination silent corruption; O(n)→O(1) tile-offset lookup
  (`cog/block_index.rs`, was 77% of one band read on an 8000-strip file)
- [x] **`oxigeo-core`**: new sealed `RasterElement` typed conversion layer;
  `RasterBuffer::convert_to` precision corruption for `UInt64`/`Int64` > 2^53; latent UB
  in `as_slice`/`as_slice_mut`/`row_slice` (unchecked alignment + dangling pointer at
  zero length); `DataSource::read_range_into`/`range_slice` fast-path trait methods
- [x] **Downstream fan-out (same defect, cool-japan/oxigeo#14)**: oxigeo-qc (big-endian +
  planar scan), oxigeo-server (WMS/WMTS overview-level + multi-band window),
  oxigeo-services WCS (multi-band truncation), oxigeo-mobile (planar tile, region read,
  overview stats), oxigeo-wasm (byte-order double-swap), oxigeo-node (multi-band open),
  oxigeo-cli (workaround removal), oxigeo-ml-foundation (dataset loader),
  oxigeo-jupyter (`%stats`), oxigeo-drivers-vrt (byte order)
- [x] **Unrelated bugs found along the way**: oxigeo-streaming `ChunkedReader` failed on
  its first call on every stream; oxigeo-mbtiles spill-file (`-wal`/`-shm`/`-journal`)
  leak; `oxigeo-ml::optimization::iterative_pruning` concurrent-call model corruption;
  `oxigeo-compress` 32-bit/wasm32 `usize` overflow in `LZ4_MAX_OUTPUT_GUESS`;
  `oxigeo-compress` failed to build for `wasm32-unknown-unknown` (ahash/getrandom)
- [x] **DEFLATE tile decoding 1.45–1.79× faster** — `oxiarc-*` 0.3.6 → 0.4.0 (two-level
  Huffman, buffered bit reader, in-place LZ77 history); GeoTIFF driver uses the new
  decompress-into-slice entry point

### Added in 0.2.2
- [x] `oxigeo::Dataset::{read_interleaved, read_interleaved_into, read_window_interleaved,
  read_window_interleaved_into, read_band_into, read_window_into, data_type}` — the
  supported replacement for the pre-0.2.2 `read_band` behaviour
- [x] `oxigeo-drivers/geotiff`: `read_band_into`/`read_band_into_typed`,
  `read_window*`, `read_bands_into_typed`/`read_window_bands_into_typed`, `byte_order()`,
  `level_size()`, opt-in `parallel` feature (rayon block-decode fan-out)

## v0.2.1 — Production-Hardening Complete, Release-Ready 2026-07-28 (branch: 0.2.1)

Workspace-wide multi-agent defect sweep across all 76 crates: **342 confirmed
defects** (47 critical / 84 high / 83 medium / 33 low), **314 fixed** across 38
crate lanes (~520 files), **79 honestly deferred** (each left with a safe typed-error
path — see "Deferred from 0.2.1 hardening" below). Gates green: `cargo fmt --check`
clean, `cargo clippy --workspace --all-features --all-targets` 0 warnings,
`cargo nextest run --all-features` 17,723 passed / 0 failed / 100 skipped,
`cargo deny check` passing. Full categorized list in CHANGELOG.md `[0.2.1]`.

### Retired in 0.2.1
- [x] **`oxigeo-kafka` retired as a project.** Crate deleted from the workspace; no
  further releases; crates.io 0.0.1 and 0.2.0 yanked. It was the workspace's sole
  mandatory C-toolchain dependency (`rdkafka-sys` → `cmake` → librdkafka), against the
  Pure Rust Policy, at 4,831 lines (0.62% of ~778k) with **zero in-workspace reverse
  dependencies**. The `kafka` features of `oxigeo-etl` and `oxigeo-workflow` and the
  `rdkafka` workspace dependency went with it. Result: `cargo check --workspace
  --all-features` no longer requires `cmake`. Kafka *metadata* enums in
  `oxigeo-workflow` (`IntegrationType::Kafka`, `MessageQueueType::Kafka`) are pure Rust
  and remain. See CHANGELOG.md `[0.2.1]` → Removed.

### Projections Expansion (100+ total)
- [ ] **(partial)** 80+ new projections toward 100+ — native catalog grew to ~24
  methods; the long tail (Van der Grinten, Winkel Tripel, Wagner, McBryde-Thomas, …)
  is still reachable only via the external oxiproj PROJ-string engine
- [x] Equidistant Conic, Sinusoidal, Mollweide, Robinson, Eckert IV/VI — native
  forward/inverse + round-trip tests (`transform/conic.rs`, `transform/pseudocylindrical.rs`)
- [x] Cassini-Soldner, Gauss-Kruger extended zones — `transform/cylindrical.rs` +
  DHDN zone-3 test
- [x] EPSG expansion to 500+ definitions (added 300+ extended definitions in oxigeo-proj/src/epsg/extended.rs)
- [ ] **(partial)** Grid shift files OSTN15/RGF93/DHDN — a real NTv2 `.gsb` parser +
  `GridRegistry`/`with_hgrid` loading path now exists (users supply real grid bytes);
  bundled OSTN15/RGF93/DHDN data and automatic EPSG grid-operation selection are still
  Helmert-parameter approximations, not shipped `.gsb` interpolation

### JPEG2000 Tier-2
- [ ] **(partial)** Tier-2 packet decoder (layer/resolution/component/position) —
  real progression-order-driven packet demux (`tier2/{packet,progression,tile}.rs`)
  now wired into the reader for single-quality-layer streams; multi-layer
  (`num_layers>1`) is rejected with a typed `UnsupportedFeature`
- [ ] Rate control and quality layers — `tier2/rate_control.rs` exists but is not yet
  wired into reader/writer
- [ ] ROI (Region of Interest) support — `tier2/roi.rs` exists but is not yet invoked
  from the decode/encode path
- [ ] JPEG2000 Part 2 extensions (JP2 boxes)

### GPU Expansion
- [x] Additional compute shaders for raster operations (reproject/raster_algebra/hillshade WGSL)
- [x] GPU-accelerated reprojection (`oxigeo-gpu/src/reprojection.rs` + shader)
- [x] GPU raster algebra evaluation (`oxigeo-gpu/src/algebra.rs` + shader)
- [x] Multi-GPU workload distribution improvements (`oxigeo-gpu/src/multi_gpu.rs`)
- [x] WebGPU compute shader compilation for WASM (compile-time `ShaderRegistry`)

### ML Pipeline Enhancements
- [x] ONNX model hot-reload (`oxigeo-ml/src/hot_reload.rs` — file-watch + atomic swap)
- [x] Inference caching with content-addressed storage (SHA-256 key + LRU, `inference_cache.rs`)
- [x] Batch prediction with adaptive batch sizing (`batch/dynamic.rs`)
- [x] Model versioning and A/B testing (`model_versioning.rs` — deterministic traffic split)
- [x] Foundation model fine-tuning workflows (`oxigeo-ml-foundation` transfer/fine-tuning strategies)

### Test Coverage Expansion — DONE, all targets exceeded as of 0.1.7
(measured 2026-07-17 via `cargo test -p <crate> --all-features -- --list | grep -c ': test$'`)
- [x] oxigeo-node: 5 → 50+ tests (measured: 60)
- [x] oxigeo (umbrella): 8 → 50+ tests (measured: 257)
- [x] oxigeo-jupyter: 33 → 60+ tests (measured: 105)
- [x] oxigeo-services: 34 → 60+ tests (measured: 608)
- [x] oxigeo-metadata: 38 → 60+ tests (measured: 162)
- [x] Target: 10,000+ total tests (workspace at ~16,775 tests as of 0.1.7 production-hardening, see header above)

### Format Driver Improvements
- [x] GeoTIFF: JPEG-in-TIFF decompression, LERC codec (`jpeg_codec.rs`; `lerc_codec/lerc2.rs` real BitStuffer2 v1/v2/v3 decode)
- [x] GeoParquet: nested geometry encoding, partitioned datasets (extended-WKB; Hive + spatial partitioning)
- [ ] **(partial)** Zarr v3: full sharding codec with partial chunk reads — the
  ZEP-0002 sharding codec (`sharding.rs`) is real, but `reader/v3.rs` still fetches the
  entire shard file before extracting one inner chunk (no range-based partial fetch)
- [x] GRIB2: template-based product definition expansion (`templates.rs`, PDT 0.0–0.48)
- [x] NetCDF: CF conventions v1.11 full compliance (`cf_conventions/v1_11.rs`)

### API Ergonomics
- [x] `oxigeo::open()` universal format detection — implemented (`crates/oxigeo/src/open.rs`: cloud-scheme → magic-byte → extension detection, returns `OpenedDataset`)
- [x] Builder pattern for all readers/writers (`crates/oxigeo/src/builder.rs` — `DatasetOpenBuilder`/`DatasetCreateBuilder`)
- [x] Streaming iterator API for large datasets (`crates/oxigeo/src/streaming.rs` — `FeatureStream`/`TileStream`/`StreamingExt`)
- [ ] **(partial)** Unified error context with source file/line — `ErrorContext`
  (category/details/path/operation/parameters) exists, but it captures the dataset path,
  not Rust `#[track_caller]`/`file!()`/`line!()` source locations

---

## Deferred from 0.2.1 hardening (roadmap → v0.3.0)

The 79 deferred findings all have safe typed-error paths today — a loud
`Unsupported*`/`NotImplemented`/`DecodingError`, never silent or fabricated data.
Below is the de-duplicated, actionable synthesis plus the large cross-crate passes
the Opus critic flagged.

### Large cross-crate passes (do as single careful sweeps)
- [ ] `#[non_exhaustive]` on the ~62 public error enums that lack it (semver stability;
  reconcile every downstream match arm in one pass) + a `cargo-semver-checks` baseline
- [ ] Library `println!`/`eprintln!` → `tracing` migration (~624 calls in lib src:
  core 25, gpu 24, ml 11, …) + a lib-scoped `print_stdout`/`print_stderr` clippy lint
- [ ] `include`/`exclude` in all 75 crate manifests to bound published tarballs
  (0 of 75 currently declare them) + a `cargo publish --dry-run --list` size/content gate
- [ ] Route header-driven allocation through a shared `oxigeo-core` `read_range`/bounded
  helper (the 0.2.1 caps covered in-crate driver buffers only)
- [ ] Sweep the remaining `oxigeo-drivers/*` parsers (jpeg2000, lerc, and the NTv2
  `.gsb` parser — confirmed overflow via fuzz) for the same header-driven-allocation pattern

### Format drivers
- [ ] JPEG2000: multi-layer (`num_layers>1`) Tier-2 decode, custom/variable precinct
  sizes, ROI (RGN/MaxShift), 9/7 irreversible (lossy) wavelet, GeoJP2 IFD→geotransform
  extraction, and consolidation of the three divergent JP2 box parsers
- [ ] GeoTIFF: LERC *encode* (BitStuffer2/Huffman), full old-style JPEG (`compression=6`)
  TN2 reconstruction + `jpeg` on by default, pure-Rust LZMA (34925) / JPEG XL (50002) decoders
- [ ] GRIB2: DRT 5.41 (PNG) and 5.42 (CCSDS AEC) decode; validate the rotated-grid
  (GDT 3.1) nonzero-angle path against real vectors
- [ ] **(partial)** HDF5/NetCDF: chunked/compressed write landed in 0.2.1 (the
  `oxigeo-drivers/hdf5` writer no longer silently drops chunking/compression/fill-value
  hints — honest errors for shapes `oxih5` cannot represent — and object-header parsing
  is real, so `decode_chunk`/filter-pipeline/chunking are no longer dead code); still open:
  big-endian source normalization, V2 object headers + Data Layout v4 chunk metadata, real
  SWMR/VDS and write-side group symmetry + scale/offset auto-apply (the last three need
  upstream oxih5/oxinetcdf changes)
- [ ] FlatGeobuf: curved geometry types (CircularString/CompoundCurve/CurvePolygon/
  MultiCurve/MultiSurface) via arc densification; a true streaming parser for large
  standard (non-NDJSON) GeoJSON
- [ ] GeoPackage: arbitrary-WKB + typed attribute columns + multi-page tables
  (writer is currently 2-D point / fid+geom only); COPC: LAZ point formats 6–10
  (LASzip contextual decoder) and full-waveform `.wdp` sample fetch

### CRS & algorithms
- [ ] Real NEON SIMD for the remaining 8 `_simd` algorithm files (projection/terrain/
  focal/colorspace are naturally vectorizable; histogram/texture scatter and
  cost-distance/hydrology graph are harder) — currently honest scalar with a status note

### Server & OGC services
- [ ] Mount `oxigeo-services` OGC endpoints (WFS/WCS/WPS/CSW/Features/Tiles) inside
  `oxigeo-server` — needs a root workspace dependency + feature/coverage-source adapters
- [ ] CQL2 spatial (`S_INTERSECTS`/…) and temporal (`T_AFTER`/…) predicates; consolidate
  the two CQL implementations (`ogc_features::cql` vs `wfs/database::CqlFilter`)
- [x] `oxigeo-gateway`: **serving layer DONE in 0.2.1.** The previously stubbed
  `Gateway::serve()` (accepted TCP connections, no-op `handle_connection`) is now a real
  axum 0.8 HTTP service via a new `GatewayServer`/`GatewayServerBuilder`. Routes:
  `GET /health`, `GET /gateway/metrics`, `POST /graphql` (+ GraphiQL when introspection
  enabled, `/graphql/ws` subscriptions when `enable_subscriptions`), `GET /ws` WebSocket
  upgrade, and a load-balanced reverse-proxy fallback (streaming hyper 1 client, Pure-Rust
  OxiTLS HTTPS upstreams, hop-by-hop stripping, `FailoverManager` retries honoring
  `retry_attempts`, circuit breaking, per-attempt timeouts). Pipeline: query-free trace
  spans, version negotiation + deprecation headers, in-house middleware chain (CORS +
  `OPTIONS` preflight, real `Accept-Encoding` compression, real LRU+TTL response caching,
  logging, metrics), JWT/API-key/session auth (+ `require_auth`/`require_mfa` enforced),
  atomic rate limiting, body/timeout limits, and a `require_permission` RBAC route guard.
  Honesty fixes: real `CachingMiddleware` (was a no-op), the orphaned 1,865-line
  `middleware::advanced` module un-orphaned/compiling/tested, and `enable_subscriptions` /
  `retry_attempts` / `require_mfa` / `enable_websocket` flags now actually enforced. Crate
  tests: 266 → 381 (+ 3 doctests).
  - v0.3.0+ follow-ups: WebSocket pass-through proxying; upstream keep-alive connection
    pooling; response-side (upstream→client) transformation wiring (request-side only
    today); GraphQL resolvers backed by real storage (currently serve demo/in-memory
    data); middleware-chain hops and proxied requests are buffered (bounded by
    `max_body_size`) while proxy responses stream

### ML, cloud & connectors
- [ ] ML: consolidate the duplicate model-versioning systems, re-enable the `temporal`
  feature, harden DirectML COM `QueryInterface` (Windows-only), wire `FineTuningScheduler`
  into `Trainer.train()`, and replace the batch-norm/bottleneck approximations
- [ ] Azure/GCP data-plane ops left as typed `NotImplemented` (Monitor metric/diagnostic
  ingestion, Cost alert/export, Synapse `execute_query`, ML `invoke_endpoint`, GCP cost
  forecast/export) — each needs a per-region/data-plane endpoint the control-plane can't mint
- [ ] OAuth2/SAS auto-refresh for S3/GCS/Azure backends (HttpBackend only today),
  anonymous S3 (`AWS_NO_SIGN_REQUEST`), Python GCS/Azure per-call auth + `driver="VRT"`,
  and credit-based backpressure + exactly-once (Kinesis/PubSub) wiring into the broker crates

### Bindings & platform
- [ ] `oxigeo-mobile`: Android `nativeReadTile` JNI export; build-verify iOS/Android objc
  paths (no toolchain in CI); the Jupyter kernel is still a toy evaluator (honest error);
  fix upstream `oxigeo-geotiff` `read_band(band)` ignoring its `band` argument
- [ ] no_std reach: `oxigeo-core` serde_json/time conversions std-only under no_std+alloc
  (needs a workspace `serde_json` alias); `oxigeo-offline` wasm-only build fails on an
  unconditional `tokio::sync::RwLock`; WASM Component Model (wasip2) general CRS
  transforms; RISC-V/Redox CI verification; Python 3.13 free-threaded testing;
  `PowerManager` default `NoController` remains bookkeeping-only (needs vendor HAL)

### Soundness, testing & supply-chain (Opus critic)
- [ ] Commit a golden interop corpus of small GDAL/QGIS-produced files per format with
  value assertions — every driver test currently round-trips only its own output
- [ ] Add `# Safety` invariants to the 818 unsafe blocks (~123 documented) and add a
  Miri/ASAN gate over the parser/SIMD/GPU/mmap paths
- [ ] **(partial)** Fuzzing blind spots: 7 new libFuzzer targets landed in 0.2.1 (NetCDF,
  HDF5 superblock/object-headers, VRT XML, GeoJSON, and more — 11 format/parser targets
  total); still open: J2K-codestream/DBF/NTv2-grid targets, seeding the targets that ship
  no corpus, and routing the confirmed upstream `oxih5` overflow panics to the oxih5
  project
- [ ] **(partial)** `deny.toml` (advisories + bans + licenses) is now committed and wired
  into `cargo deny check`, and a 75-crate topological publish-order manifest is now in-repo
  (both landed in 0.2.1); still open: a feature-powerset build (`cargo hack`), an all-crate
  MSRV check, and assembling the above into one local release gate; make writers
  reproducible (injectable deterministic clock for gpkg/hdf5/pmtiles timestamps); add
  encoding/locale round-trip tests and byte-offset/field context to driver parser errors

---

## v0.1.7 — Production-Hardening Complete, Released 2026-07-20 (tag: v0.1.7)

- [x] `oxigeo-cloud-enhanced`: real Azure IMDS managed-identity tokens (`azure_identity::ManagedIdentityCredential`) replacing placeholder-token stub
- [x] `oxigeo-cloud-enhanced`: real GCP metadata-server access/identity tokens + IAM Credentials impersonation (`GCE_METADATA_HOST` overridable, mock-server tests); `reqwest` optional under `gcp` feature
- [x] `oxigeo-cloud`: multicloud `build_backend()` factory (S3/GCS/AzureBlob/Http, feature-gated) + backend cache; `get`/`put`/`delete`/`exists_in_provider` now functional
- [x] `oxigeo-drivers-advanced`: JPEG2000 decode delegates to `oxigeo-jpeg2000` (real decode, full header parse) instead of gray placeholder pixels; `jpeg2000` feature now dep-gated
- [x] `oxigeo-services`: WFS-T Memory/File transactions fully implemented (insert/update/delete/replace, per-path write serialization)
- [x] `oxigeo-services`: Database transactions/feature-sources/SQL count behind new non-default `postgis` feature (`oxigeo-postgis` pool, `ST_GeomFromGeoJSON`/`ST_AsGeoJSON`)
- [x] `oxigeo-services`: WCS File/Url/Memory coverages real (GeoTIFF read/write via `oxigeo-geotiff`), Url fetch behind new non-default `remote` feature; `encode_as_geotiff` produces real GeoTIFF bytes
- [x] `oxigeo-ml-foundation`: `onnx_export.rs` — pure-Rust ONNX protobuf encoder (ir_version 8, opset 13), round-trip-validated against `oxionnx`
- [x] `oxigeo-ml-foundation`: honest typed errors for unavailable scirs2 input gradients (no more silent zero gradients); weights save/load via `oxicode`; augmentation noise now real Gaussian sampling (`scirs2_core` seeded RNG)
- [x] `oxigeo-workflow`: Temporal/Prefect `import_workflow` round-trips exporter-generated definitions (metadata headers for lossless ID recovery); export emits real activity bodies
- [x] `oxigeo-etl`: `transform_crs` implemented via `oxigeo_proj::transform_epsg` (spawn_blocking to avoid nested-runtime panic — real bug fix, previously panicked inside any tokio runtime); `calculate_bbox` fixed (was returning `[0,0,0,0]`); `calculate_ndvi` implemented (zero-denominator guard)
- [x] `oxigeo-cli`: `info`/`stats` implemented for FlatGeobuf, GeoParquet, Zarr, GeoPackage, JPEG2000, COPC, PMTiles, MBTiles (was "not yet implemented"); `merge` placeholder test replaced with real assertion
- [x] `oxigeo-algorithms`: Lanczos resampling Wrap & Mirror edge modes implemented (`rem_euclid` / reflect-101)
- [x] `oxigeo-gpkg`: tile matrix set `srs_id` now writes real 4326 via new `int2_st()` (was placeholder `4`)
- [x] `oxigeo-geojson-stream`: TopoJSON writer now emits real arcs for LineString/MultiLineString (open-chain topology: endpoint junctions, no-rotation splitting, shared-arc dedup with negative reversed indices) — was `"arcs": []` stub
- [x] `oxigeo-gpu`: subgroup/warp operations emit native WGSL subgroup builtins with workgroup-shared-memory emulation fallback; Metal filter/reduction/nn shader generators implemented; ballot/vote/`SimdGroupOperations` upgraded; new execute-and-compare GPU tests (verified on Metal)
- [x] `oxigeo-bench`: raster/io scenarios now do real work (tile reads, `MmapDataSource`)
- [x] `oxigeo-wasm`: new browser APIs — `WasmCogViewer.openBytes` (drag-drop local GeoTIFF with full codec support incl. LZW/Zstd via `CogReader<MemorySource>`), `readTileElevation` (SampleFormat tag 339 parsing), `WasmTerrain` (hillshade/multidirectional/slope/aspect/color-relief-shaded — Horn method, `ImageData` output), `WasmProjection` + `wgs84ToWebMercator`/`webMercatorToWgs84` shims
- [x] GeoLab demo: `demo/cog-viewer` rebranded OxiGeo GeoLab, drag-drop, terrain-analysis panel, honest byte counters, all CDN deps vendored locally; staged to cooljapan.tech/geolab/ (deploy manual)
- [x] `oxigeo-server`: new example `render_hero.rs` (DEM → combined_hillshade → colormap → PNG)
- [x] README: compile-correct quickstart (`crs()` Option), refreshed stats, fixed doc links, clickable GeoLab hero + `## Demo` section with native-render gallery (`docs/media/`)
- [x] New: `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`; docs.rs metadata on all 64 remaining publishable crates (21 curated for Pure-Rust docs builds)
- [x] Examples/benches: 31 orphaned top-level examples wired into `oxigeo-examples` (API rot fixed, 5 dupes pruned), 11 benches wired into `oxigeo-bench`
- [x] Hygiene: removed rustc-ICE dump, auto-fix logs/backups, 3 stray `.bak` files in crate `src` trees; `.gitignore` hardened; `.cargo/config.toml` stale rusqlite/proj-sys entries removed; `pypi-publish.yml` stale openssl-devel step removed; `pyproject.toml` + `package.json` synced to 0.1.7
- [x] `oxigeo-security`: `enterprise`/`tls`/`attestation` feature split (default = all three; wasm32-clean attestation-only surface) + new `attestation` module — domain-separated blake3 hash chain → Merkle root/proofs → Ed25519 seal, full re-verification from attestation JSON alone, native `verify_attestation.rs` example
- [x] `oxigeo-wasm`: `sentinel` module (Earth Search STAC pair search, self-contained UTM↔WGS84, NDVI change-detection pipeline with Otsu/polygonize/Karney hectares), `vault` module (hash-chained session log + Ed25519 seal), `anomaly` module (Z-score/IQR/modified-Z/percentile ports); COG reader: overview-level reads (`read_tile_level`, `read_window_u16`/`read_window_rgb8`), PREDICTOR=2 undo, BitsPerSample/SampleFormat array-parse fix
- [x] `oxigeo-geoparquet`: `plan_pushdown()`/`execute_pushdown()` APIs (metadata-only planning + `ChunkReader` execution), GeoParquet 1.1 `covering.bbox` + plain-`bbox` struct detection, `AttributeFilter::Cmp` with Int64/Float64 literal coercion, multi-filter conjunctions
- [x] New crate `oxigeo-wasm-geoparquet`: browser range-request GeoParquet client (sparse `ChunkReader`, 64 KiB range coalescing, SQL `WHERE` lowering via sqlparser, `RecordBatch`→GeoJSON) — npm `@cooljapan/oxigeo-geoparquet`
- [x] Three new demos staged (deploy manual): GeoSentinel (cooljapan.tech/geosentinel/), GeoVault (/geovault/ — CSP zero-egress + independent verifier), GeoParquet Live (/geoparquet/ — live 5.9 GB VIDA dataset); GeoLab pkg refresh; README `## Demos` sections for all four

### Demo-campaign follow-ups (added 2026-07-13)

- [ ] `oxigeo-query`: feature-gate tokio so the SQL engine is consumable from wasm32; until then GeoParquet Live ships its own SQL `WHERE`-fragment lowering (`crates/oxigeo-wasm-geoparquet/src/filter_expr.rs`) as a deliberate workaround
- [ ] `oxigeo-geoparquet`: Overture and other zstd-compressed GeoParquet remain out of scope until a pure-Rust parquet zstd path exists (`zstd-sys` is C FFI — forbidden by Pure Rust Policy; snappy via pure-Rust `snap` is the supported codec)
- [ ] `oxigeo-wasm`: parallelize per-window tile fetches in the COG reader (verified 2026-07-16: `detect_changes` in `crates/oxigeo-wasm/src/sentinel/core.rs` still awaits `red_a`/`nir_a`/`red_b`/`nir_b` sequentially; currently sequential; observed sentinel-cogs S3 latency variance of 23 s – 250 s on cold reads makes serial fetches the long pole)
- [x] `oxigeo-wasm`: `GeoSentinel.trueColorRgba` returns a bare RGBA buffer — should return dimensions alongside the pixels (callers currently need a separate `overlayInfo` call) — done: `true_color_rgba()` (`crates/oxigeo-wasm/src/sentinel/core.rs`) now returns a `TrueColorImage` carrying `width`/`height` alongside the pixel buffer
- [x] `oxigeo-geoparquet`: `ScalarValue::Bool` predicates are stats-only — boolean scalars surface a type-mismatch error at execution time; implement boolean-column evaluation — done: `eval_scalar_comparison` (`crates/oxigeo-drivers/geoparquet/src/predicate.rs`) now evaluates `ScalarValue::Bool` against `Boolean` columns via the Arrow comparison kernel
- [ ] `demo/cog-viewer/examples.json`: stale remote URLs — the `elevation-tiles-prod` S3 bucket no longer sends CORS headers; refresh the example COG list (candidates: sentinel-cogs and other CORS-enabled open buckets)

### Known limitations carried forward

- [x] `oxigeo-python` `numpy.rs:586`: complex dtype not yet supported — done: `to_numpy_complex()` maps `CFloat32`/`CFloat64` buffers to native `numpy.complex64`/`numpy.complex128` (round-trip tested)
- [x] `oxigeo-proj`: `transform_epsg` opens the PROJ DB per-coordinate call — inefficient for bulk transforms; needs a reusable/cached `Transformer` path — done: opt-in `TransformerCache`/`TransformerKey` (`crates/oxigeo-proj/src/cache.rs`) provides an LRU-cached, thread-safe reusable transform path
- [x] `oxigeo-stac`: implicit `reqwest` feature should be promoted to a named feature (currently pulled in without an explicit flag) — done: `async` is now the real feature gating `dep:reqwest`; `reqwest` is kept only as a backwards-compatible alias
- [x] `oxigeo-cloud-enhanced`: docs.rs build shows only the default feature surface (Azure/GCP feature-gated APIs not visible in generated docs) — done: `[package.metadata.docs.rs]` now sets `features = ["aws", "azure", "gcp"]` so the full API surface is documented

---

## v0.1.7 production-hardening (2026-07-16) [COMPLETE]

Parallel multi-lane defect-sweep campaign following the Beyond-GeoLab campaign
(2026-07-13): **233 verified defects fixed across 69 crates**. Workspace green:
`cargo check` / `cargo clippy --workspace --all-features --all-targets` / `cargo fmt --check`
all clean, ~16,775 tests passing with 0 failures. See CHANGELOG.md's
"Fixed (production-hardening campaign, 2026-07)" entry for the full categorized list
(format drivers / algorithms / security / cloud & infra / bindings / no_std & platform).

Headline fixes:

- [x] GeoTIFF floating-point predictor (`Predictor=3`) decode + encode implemented (was a silent no-op corrupting float COG round-trips)
- [x] JPEG2000 MQ-decoder `INITDEC` brought into ITU-T T.800 Annex C spec conformance
- [x] GML `srsDimension` attribute parsing (3D geometries no longer silently treated as 2D)
- [x] VRT `FirstValid` multi-byte-sample compositing fix + `BandMath` `B10`+ variable substitution
- [x] Raster/DSL calculator NaN-safe optimizer (`x * 0` no longer discards NoData/Inf semantics)
- [x] Weiler-Atherton concave fallback made non-silent (mismatch surfaced rather than masked as a plausible result)
- [x] RBAC `resource_pattern` now enforced (was a privilege-widening bug — parsed but never consulted)
- [x] TOTP constant-time verify + ±1 time-step clock-skew tolerance window (RFC 6238 §5.2)
- [x] SQL `GROUP BY` executor implemented in `oxigeo-query` (was a no-op)
- [x] Umbrella `DatasetWriter::finalize()` now writes real formats or a typed error (was a fake `OXIG`-prefixed placeholder blob)
- [x] `server.toml` now actually loaded in Docker/k8s via `OXIGEO_CONFIG` (was parsed then discarded)
- [x] `oxigeo-stac` feature wiring fixed — no longer pulls `aws-lc-sys` for consumers who never use the async surface
- [x] Kafka/Kinesis streaming commit-strategy & consumer-lease correctness fixes
- [x] `oxigeo-core` (no_std) now compiles under `--no-default-features --features alloc`
- [x] `oxigeo-drivers/hdf5` and `oxigeo-netcdf` re-backed by the real Pure-Rust `oxih5 0.1.4`
  / `oxinetcdf 0.1.4` crates (crates.io, no libhdf5/libnetcdf FFI) — replaces the legacy
  `OXIGDAL_HDF5_METADATA_V1` JSON-sidecar placeholder (0.1.x-era on-disk identifier,
  removed) that returned zeros for real `.h5`
  files; `oxigeo-netcdf` now reads genuine NetCDF-4/CF files. Public API unchanged
  (`Hdf5Reader::open`, `Attribute`/`AttributeValue`/`Datatype`/`Hdf5Version`/`Hdf5Writer`,
  `NetCdfReader::open`); 730 tests passing across the 4 affected crates, clippy clean

### Deferred to v0.2.0 (honest limitations)

The current code has safe error paths and correct fallbacks for all of the following — not
silent corruption — but the full fix is architecturally larger than this campaign's scope:

- [ ] **HDF5 v2/v3 superblock reading**: `oxih5` 0.1.4 fully reads v0-superblock `.h5` files;
  v2/v3-superblock files currently open but yield an empty tree (best-effort, never faked as
  populated data)
- [ ] **HDF5 big-endian source normalization**: `oxih5` 0.1.4 does not yet normalize
  big-endian-encoded source datatypes on read
- [ ] **HDF5 chunked/compressed write**: `oxih5` 0.1.4's writer produces contiguous real HDF5
  only — chunk and compression hints passed by callers are accepted but dropped (written
  values are correct, layout is not chunked/compressed)
- [ ] **NetCDF sub-group flattening**: `oxinetcdf` 0.1.4's reader surfaces the root group
  only; nested NetCDF-4 sub-groups are not yet flattened/exposed
- [ ] **NetCDF auto scale/offset/fill application**: `scale_factor`/`add_offset`/`_FillValue`
  are exposed as attributes but not automatically applied to decoded values (unchanged
  contract — same as prior releases)
- [ ] **JPEG2000 Tier-2**: packet/precinct/layer decode is not yet invoked by the reader —
  code-block bytes are still sliced by naive even division rather than driven by the real
  progression order (LRCP/RLCP/RPCL/PCRL/CPRL) from the COD marker. Tier-1 (EBCOT/MQ) decode
  is real; Tier-2 packet assembly, rate control, and ROI support are not
- [ ] **FlatGeobuf**: header and feature geometry use a custom ad-hoc binary layout, not real
  FlatBuffers — files are not interoperable with the upstream `flatgeobuf.fbs` schema
  (`header.fbs` + `feature.fbs`) despite the pure-Rust `flatbuffers` crate already being a
  workspace dependency
- [ ] **oxigeo-ml multi-channel tensors**: `buffer_to_ndarray()` cannot produce a
  multi-channel tensor because `RasterBuffer` is architecturally single-band; `Model::predict`
  / `predict_batch` do not yet accept `MultiBandBuffer` and stack per-band slices into a real
  `[1,C,H,W]` tensor (nor is there a symmetric multi-band `ndarray_to_buffer`) — the fix
  ripples across the trait and multiple call sites
- [ ] **oxigeo-python GIL release**: remaining `#[pyfunction]`/`#[pymethods]` entry points
  still hold the GIL for their full duration — `operations.rs` (`read`/`read_bands`/`write`/
  `clip`/`merge`/`translate`/`build_overviews`) and `algorithms.rs`
  (`histogram`/`gaussian_blur`/`median`/...) still need `Python::allow_threads` wrapping
- [ ] **Root integration-test suite**: 21 quarantined tests across 7 of 11 root-level
  integration files (~4,280 lines) exercise local stand-in parsers/fixtures instead of the
  real driver crates (NetCDF, HDF5, Shapefile, GeoParquet, and others) and are not
  compiled/run by `cargo test`/`nextest`
- [ ] **PowerManager SoC-specific power states**: `oxigeo-embedded`/`oxigeo-noalloc`
  `PowerManager::apply_*()` for HighPerformance/Balanced/LowPower/UltraLowPower/DeepSleep
  remain bookkeeping-only no-ops — CPU frequency scaling and peripheral clock/power gating are
  inherently SoC-specific (vendor clock/power controllers, not the ARM/RISC-V ISA) and cannot
  be implemented generically without per-vendor HAL crates. (Note: this is distinct from the
  no_std *build* itself, which is green — see "`oxigeo-core` ... now compiles" above.)
- [ ] **LERC2**: bit-stuffed external-decode path is not yet implemented
- [ ] **Publish-script verification**: MSRV 1.89 claim and crate-publish ordering/dry-run gate
  for `~/work/pub_oxigeo.sh`'s CRATES array remain unchecked (cross-repo, outside this
  workspace's ownership)

---

## v0.1.6 — Previous Release (2026-06-15) [COMPLETE]

- [x] Pure-Rust SQLite migration: `rusqlite`/`libsqlite3-sys` (C FFI) fully replaced by `oxisql-sqlite-compat 0.1.5` (Limbo engine) across db-connectors, gpkg, drivers-advanced, mbtiles, pmtiles
- [x] Policy fixes: `ring`, `rusqlite`, `rdkafka-sys` removed from default feature closure
- [x] native-tls → oxitls migration (pure Rust TLS stack)
- [x] ~35 inline deps migrated to `*.workspace = true`
- [x] `oxigeo-shapefile`: non-UTF-8 DBF encoding via `encoding_rs` (CPG/LDID support, PR #10)
- [x] `oxigeo-proj`: `wkt_to_proj_string()` — WKT→PROJ conversion (PR #9)
- [x] `oxigeo-cache-advanced`: W-TinyLFU + Count-Min Sketch cache eviction
- [x] `oxigeo-copc`: LiDAR waveform point formats 9/10 (`WaveformPacket`)
- [x] `oxigeo-drivers/hdf5`: HDF5 v2/v3 superblock parser + Jenkins hash
- [x] `oxigeo-index`: Delaunay triangulation (`triangulate()`, `Triangulation::convex_hull()`)
- [x] `oxigeo-qc`: Batch QC runner, GPKG/STAC/radiometric validators, per-sensor band ranges
- [x] `oxigeo-sensors`: Gaussian Maximum Likelihood Classifier
- [x] `oxigeo-streaming`: OxiStore-backed persistent `KvStateBackend`
- [x] `oxigeo-terrain`: GLCM texture derivatives, TPI variants, geomorphons, cost-distance/least-cost-path
- [x] `oxigeo-temporal`: Whittaker smoother + Savitzky-Golay filter for gap filling
- [x] `oxigeo-analytics`: permutation significance testing for Local Moran's I
- [x] `oxigeo-metadata`: DOI/INSPIRE metadata transform
- [x] Umbrella: GPX, KML, TopoJSON format support in `open()` / vector streaming
- [x] Dependency upgrades: scirs2 0.4.4→0.5.0, oxionnx 0.1.3→0.1.4, oxiarc 0.3.0→0.3.3, oxicode 0.2.3→0.2.4

---

## v0.1.5 — Previous Release (2026-05-22) [COMPLETE]

- [x] `oxigeo-gpu`: WGSL `RayMarchUniforms` layout fix — removed stray `_pad1: f32` that shifted every field by 4 bytes and caused the Metal compute kernel to read `max_steps` ≈ 1.05×10⁹, hanging `device.poll(wait_indefinitely)` for 120s+. Previously-timing-out `test_ray_march_gpu_matches_cpu_when_backend_present` now passes in 0.127s.

---

## v0.1.4 — Previous Release (2026-04-19) [COMPLETE]

- [x] Wave 1: Weiler-Atherton polygon clipping, Karney geodesic area, DE-9IM topology, marching squares contour extraction
- [x] Wave 1: ML migration ort → oxionnx (Pure Rust ONNX runtime)
- [x] Wave 2: R-tree enhancements (deletion, STR bulk load, k-NN priority queue, serialization)
- [x] Wave 2: SIMD resampling (AVX2+NEON), raster polygonization, topology-preserving simplification
- [x] Wave 2: NoAlloc geometry types (FixedLineString, FixedRing, BBox3D, Mercator, geohash neighbours)
- [x] Wave 2: PMTiles reader completion (tile retrieval, OxiARC decompression, FNV-1a dedup)
- [x] Wave 2: COPC reader, GeoPackage B-tree + 3D WKB
- [x] Fixes: pyo3 0.28 migration in oxigeo-python, geojson-stream test clippy cleanup

---

## v0.1.3 — Previous Release (2026-03-21) [COMPLETE]

- [x] Fixed wgpu 29 API breaking changes (Instance::new, bind_group_layouts)
- [x] Fixed libsqlite3-sys version conflict (rusqlite 0.37, proj-sys compat)
- [x] Fixed macOS librocksdb-sys DYLD rpath via .cargo/config.toml
- [x] Fixed 6 critical oxiarc-brotli bugs (patched via [patch.crates-io])
- [x] Fixed pipeline_builder.rs clippy redundant closure

---

## v0.1.2 — Previous Release (2026-03-17) [COMPLETE]

- [x] WASM enhancements and optimizations
- [x] npm publishing workflow for WASM bindings
- [x] Code growth to 540K SLoC (1,934 .rs files)

---

## v0.1.1 — Previous Release (2026-03-11) [COMPLETE]

### Core & Algorithms
- [x] Core geospatial types, traits, async I/O, Arrow buffers, no_std core
- [x] SIMD-optimized raster algorithms (AVX2, AVX-512, NEON)
- [x] Vector algorithms (topology, buffering, convex hull, spatial joins)
- [x] Raster algebra DSL (Pest grammar parser)
- [x] DSL statistical functions (median, mode, percentile)
- [x] DSL for-loop support with 1M-iteration OOM guard
- [x] calculator.rs refactor: 7 modules (ast/lexer/parser/optimizer/evaluator/ops/mod)
- [x] Terrain analysis: hillshade, slope, aspect, curvature, TRI, TPI, roughness

### CRS & Projections (oxigeo-proj)
- [x] Pure Rust PROJ: 20+ projections (UTM 1-60, Web Mercator, LCC, Albers, etc.)
- [x] 211+ EPSG definitions (all UTM zones, JGD2011, GDA2020, CGCS2000, polar)
- [x] WKT2 parser (ISO 19162:2019) with WKT1/ESRI WKT backward compatibility
- [x] Datum transformations (Helmert, Molodensky, NTv2, NADCON)
- [x] SIMD-vectorized batch transforms

### Format Drivers (11 formats)
- [x] GeoTIFF/COG: BigTIFF, overviews, DEFLATE/LZW/ZSTD/JPEG, float predictor
- [x] GeoJSON: RFC 7946, streaming, GeoArrow zero-copy
- [x] GeoParquet: Arrow-native, spatial predicate pushdown
- [x] Zarr v2/v3: sharding, codec pipeline, consolidated metadata
- [x] FlatGeobuf: Hilbert R-tree spatial indexing
- [x] Shapefile: SHP/SHX/DBF full attribute table
- [x] NetCDF: CF conventions, unlimited dims, groups
- [x] HDF5: chunking, compression, SWMR protocol
- [x] GRIB1/2: parameter/level tables
- [x] JPEG2000: tier-1 EBCOT decoder (MQ coder, 3-pass)
- [x] VRT: band math, source mosaicking

### Cloud & Storage
- [x] S3/GCS/Azure Blob backends with HTTP range
- [x] Pure Rust compression (oxiarc-deflate/zstd/lz4/bzip2/lzw)
- [x] Multi-tier cache (in-memory LRU, disk, Redis)
- [x] Delta encoding (all 10 data types)

### Enterprise & Infrastructure
- [x] Security: AES-256-GCM, ChaCha20-Poly1305, Argon2id, RBAC/ABAC
- [x] HA: Raft consensus, failover, leader election
- [x] Observability: OpenTelemetry, Prometheus, Jaeger
- [x] OGC server: WMS 1.3.0, WFS 2.0.0
- [x] API gateway: JWT, OAuth2, rate limiting

### Platform Bindings
- [x] WASM: WasmCogViewer, Huffman compression, < 1MB bundle
- [x] Python: PyO3/Maturin, NumPy array returns
- [x] Node.js: napi-rs, CJS + ESM
- [x] iOS/Android: Swift FFI, Kotlin/JNI
- [x] Embedded: no_std, heapless, embedded-hal

### CLI
- [x] `inspect`, `convert`, `buildvrt` commands
- [x] DEM terrain: hillshade, slope, aspect, TRI, TPI, roughness

### Quality
- [x] 0 unwrap() in production code (2 in non-compiled doc comments)
- [x] 0 clippy warnings, 0 rustdoc warnings
- [x] 0 todo!()/unimplemented!() stubs (except 4 in oxigeo-python, 1 in grib)
- [x] All files < 2,000 lines
- [x] All deps via workspace inheritance
- [x] Pure Rust default features (C/Fortran feature-gated)

---

## v0.3.0 — Target: Q2 2026

### Streaming v2
- [ ] Backpressure-aware stream processing with credit-based flow control
- [ ] Session window improvements with gap detection
- [ ] Exactly-once semantics for Kinesis/Pub/Sub (Kafka dropped — `oxigeo-kafka` retired in 0.2.1)
- [ ] Stream-to-stream joins with temporal alignment
- [ ] Checkpoint-based recovery with minimal replay

### Cloud-Native Tile Server v2
- [ ] OGC Tiles API (replacing WMTS)
- [ ] OGC Features API Part 1 & 2
- [ ] Vector tile generation (MVT/Mapbox)
- [ ] Dynamic style rendering (Mapbox GL style spec)
- [ ] CDN-friendly caching headers

### Extended STAC Support
- [ ] STAC Extensions: eo, sar, view, projection, scientific
- [ ] STAC API conformance classes
- [ ] STAC collection-level aggregation
- [ ] STAC transaction extension (create/update/delete)

### Additional Formats
- [x] GeoPackage (Pure Rust SQLite reader)
- [x] MBTiles (vector tile archives)
- [x] Cloud Optimized Point Cloud (COPC)
- [x] PMTiles (single-file tile archive)

### Performance
- [ ] Adaptive tile size selection for COG
- [ ] Parallel I/O coalescing for cloud reads
- [ ] Memory-mapped file support for local reads
- [ ] Zero-copy Arrow IPC for inter-process communication

---

## v1.0.0 — Target: Q3 2026

### Stability & LTS
- [ ] Semantic versioning guarantee: no breaking changes until 2.0
- [ ] Minimum 24-month LTS maintenance commitment
- [ ] Migration guide from 0.x to 1.0
- [ ] Full API documentation with examples for every public item

### Enterprise Compliance
- [ ] SOC2 Type II audit trail
- [ ] GDPR data handling compliance documentation
- [ ] FIPS 140-2 cryptographic module validation
- [ ] FedRAMP authorization support

### Ecosystem Integration
- [ ] Conda-forge package
- [ ] Homebrew formula
- [ ] Docker Hub official images (Alpine, Debian)
- [ ] Kubernetes Helm chart for oxigeo-server
- [ ] GitHub Actions for geospatial CI/CD

### Documentation
- [ ] Complete API reference with examples
- [ ] Architecture decision records (ADRs)
- [ ] Performance tuning guide
- [ ] Cookbook: 50+ recipes for common geospatial tasks
- [ ] Video tutorials

---

## Ongoing / Cross-Cutting

### Dependency Maintenance
- [ ] **(partial)** Replace unmaintained transitive deps: `indicatif`'s `number_prefix`
  dropped via the 0.2.1 `indicatif` 0.18 bump; still open: rustls-pemfile, sled/fxhash, evcxr/json
- [ ] Track and patch security advisories within 48h
- [ ] Keep Arrow ecosystem at latest stable (currently 59, bumped in 0.2.1)
- [ ] Keep all COOLJAPAN deps (oxiarc-*, scirs2-core, oxiblas, oxicode, OxiFFT) at latest
- [ ] `oxigeo-security`: remove unused `tempfile` dev-dependency (declared in `Cargo.toml`,
  not referenced anywhere in the crate's source — missed by the 0.2.1 cargo-machete sweep)

### Code Quality
- [ ] Maintain 0 clippy warnings, 0 rustdoc warnings
- [ ] Maintain 0 unwrap() in production code
- [ ] Maintain all files < 2,000 lines
- [x] Increase test count toward 10,000+ — target exceeded (~16,775 tests as of 0.1.7 production-hardening, see "Test Coverage Expansion" above)
- [ ] Property-based testing (proptest) for core algorithms
- [ ] Fuzzing (cargo-fuzz) for format parsers (GeoTIFF, JPEG2000, GRIB)

### Platform
- [ ] RISC-V support (no_std)
- [ ] Redox OS compatibility testing
- [ ] WASM Component Model (wasm32-wasip2) support
- [ ] Python 3.13+ free-threaded mode testing
- [ ] oxigeo-mobile: Android nativeReadTile JNI export is missing (Kotlin declares 11 external funs, Rust exports 10 — pre-existing since 0.1.x); implement Java_com_cooljapan_oxigeo_OxiGeo_nativeReadTile

---

*Last updated: 2026-07-28*

## Stubs to implement (added 2026-06-12 by /cooljapan-stub-check)

- [x] `oxigeo-wasm`: `crates/oxigeo-wasm/src/tests/functions_3.rs:542` — implement full Huffman codec for wasm decompression path (or wire oxiarc equivalent)
  - Priority: P2 | Scope: medium | Hint: oxiarc
- [x] `oxigeo-wasm`: `crates/oxigeo-wasm/src/tests/functions_4.rs:249` — implement decompression roundtrip test
  - Priority: P2 | Scope: small | Hint: oxiarc
- [x] `oxigeo-python`: `crates/oxigeo-python/src/numpy.rs:586` — implement proper complex dtype mapping when pyo3 supports it — done: `to_numpy_complex()` (production-hardening campaign, 2026-07-16)
  - Priority: P2 | Scope: small | Hint: none
- [x] `oxigeo-algorithms`: `crates/oxigeo-algorithms/tests/texture_analysis_test.rs:212` — investigate GLCM computation on uniform rasters (empty matrix bug) and re-enable ignored test
  - Priority: P2 | Scope: small | Hint: none
- [ ] `oxigeo-workflow`: `crates/oxigeo-workflow/src/integrations/temporal.rs:27` — replace generated activity-logic TODO placeholder with real Temporal workflow activity bodies
  - Priority: P2 | Scope: medium | Hint: none
- [x] `oxigeo-drivers/jpeg2000`: `crates/oxigeo-drivers/jpeg2000/src/lib.rs:154` — implement JPEG2000 driver (tracked as TODO in module doc)
  - Priority: P2 | Scope: large | Hint: none

## Policy follow-ups (added 2026-06-13 by /policy-check)

### #1 No-unwrap purge — production code only (tests on infallible paths are policy-allowed)
- [x] **oxigeo**: 0 production `.unwrap()` (clean)
  - Verified: 36 src/ files scanned; all `.unwrap()` calls are inside `#[cfg(test)]` blocks.
    Cross-confirmed by checking every `.rs` file for unwraps before the first `#[cfg(test)]`
    annotation — result: zero hits. `oxigeo` is fully policy-compliant on #1.

## Stubs to implement (added 2026-06-22 by /cooljapan-stub-check)

- [x] **oxigeo** `oxigeo-python`: `crates/oxigeo-python/src/numpy.rs:586` — `TODO`: `Use proper complex dtype when pyo3 supports it` — done: `to_numpy_complex()` (production-hardening campaign, 2026-07-16)
  - **Priority:** P2  **Scope:** small  **Cross-project:** none
  - **Approach:** Map numpy complex64/complex128 arrays to the Rust Complex type instead of falling back, so complex raster/array data round-trips correctly.
  - **Risk:** pyo3 complex support is version-dependent; guard the mapping so unsupported builds give a clear error rather than corrupting real/imag layout.

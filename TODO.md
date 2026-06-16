# OxiGDAL TODO

> Version: 0.1.6 (2026-06-15) | 78 crates | 14,605 tests | ~580K Rust SLoC

---

## v0.1.6 — Current Release (2026-06-15) [DONE]

- [x] Pure-Rust SQLite migration: `rusqlite`/`libsqlite3-sys` (C FFI) fully replaced by `oxisql-sqlite-compat 0.1.5` (Limbo engine) across db-connectors, gpkg, drivers-advanced, mbtiles, pmtiles
- [x] Policy fixes: `ring`, `rusqlite`, `rdkafka-sys` removed from default feature closure
- [x] native-tls → oxitls migration (pure Rust TLS stack)
- [x] ~35 inline deps migrated to `*.workspace = true`
- [x] `oxigdal-shapefile`: non-UTF-8 DBF encoding via `encoding_rs` (CPG/LDID support, PR #10)
- [x] `oxigdal-proj`: `wkt_to_proj_string()` — WKT→PROJ conversion (PR #9)
- [x] `oxigdal-cache-advanced`: W-TinyLFU + Count-Min Sketch cache eviction
- [x] `oxigdal-copc`: LiDAR waveform point formats 9/10 (`WaveformPacket`)
- [x] `oxigdal-drivers/hdf5`: HDF5 v2/v3 superblock parser + Jenkins hash
- [x] `oxigdal-index`: Delaunay triangulation (`triangulate()`, `Triangulation::convex_hull()`)
- [x] `oxigdal-qc`: Batch QC runner, GPKG/STAC/radiometric validators, per-sensor band ranges
- [x] `oxigdal-sensors`: Gaussian Maximum Likelihood Classifier
- [x] `oxigdal-streaming`: OxiStore-backed persistent `KvStateBackend`
- [x] `oxigdal-terrain`: GLCM texture derivatives, TPI variants, geomorphons, cost-distance/least-cost-path
- [x] `oxigdal-temporal`: Whittaker smoother + Savitzky-Golay filter for gap filling
- [x] `oxigdal-analytics`: permutation significance testing for Local Moran's I
- [x] `oxigdal-metadata`: DOI/INSPIRE metadata transform
- [x] Umbrella: GPX, KML, TopoJSON format support in `open()` / vector streaming
- [x] Dependency upgrades: scirs2 0.4.4→0.5.0, oxionnx 0.1.3→0.1.4, oxiarc 0.3.0→0.3.3, oxicode 0.2.3→0.2.4

---

## v0.1.5 — Previous Release (2026-05-22) [COMPLETE]

- [x] `oxigdal-gpu`: WGSL `RayMarchUniforms` layout fix — removed stray `_pad1: f32` that shifted every field by 4 bytes and caused the Metal compute kernel to read `max_steps` ≈ 1.05×10⁹, hanging `device.poll(wait_indefinitely)` for 120s+. Previously-timing-out `test_ray_march_gpu_matches_cpu_when_backend_present` now passes in 0.127s.

---

## v0.1.4 — Previous Release (2026-04-19) [COMPLETE]

- [x] Wave 1: Weiler-Atherton polygon clipping, Karney geodesic area, DE-9IM topology, marching squares contour extraction
- [x] Wave 1: ML migration ort → oxionnx (Pure Rust ONNX runtime)
- [x] Wave 2: R-tree enhancements (deletion, STR bulk load, k-NN priority queue, serialization)
- [x] Wave 2: SIMD resampling (AVX2+NEON), raster polygonization, topology-preserving simplification
- [x] Wave 2: NoAlloc geometry types (FixedLineString, FixedRing, BBox3D, Mercator, geohash neighbours)
- [x] Wave 2: PMTiles reader completion (tile retrieval, OxiARC decompression, FNV-1a dedup)
- [x] Wave 2: COPC reader, GeoPackage B-tree + 3D WKB
- [x] Fixes: pyo3 0.28 migration in oxigdal-python, geojson-stream test clippy cleanup

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

### CRS & Projections (oxigdal-proj)
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
- [x] 0 todo!()/unimplemented!() stubs (except 4 in oxigdal-python, 1 in grib)
- [x] All files < 2,000 lines
- [x] All deps via workspace inheritance
- [x] Pure Rust default features (C/Fortran feature-gated)

---

## v0.2.0 — Target: Q1 2026

### Projections Expansion (100+ total)
- [ ] Add 80+ new projections to reach 100+ total
- [ ] Equidistant Conic, Sinusoidal, Mollweide, Robinson, Eckert IV/VI
- [ ] Cassini-Soldner, Gauss-Kruger extended zones
- [x] EPSG expansion to 500+ definitions (added 300+ extended definitions in oxigdal-proj/src/epsg/extended.rs)
- [ ] Grid shift files: OSTN15 (UK), RGF93 (France), DHDN/ETRS89 (Germany)

### JPEG2000 Tier-2
- [ ] Tier-2 packet decoder (layer/resolution/component/position progression)
- [ ] Rate control and quality layers
- [ ] ROI (Region of Interest) support
- [ ] JPEG2000 Part 2 extensions (JP2 boxes)

### GPU Expansion
- [ ] Additional compute shaders for raster operations
- [ ] GPU-accelerated reprojection
- [ ] GPU raster algebra evaluation
- [ ] Multi-GPU workload distribution improvements
- [ ] WebGPU compute shader compilation for WASM

### ML Pipeline Enhancements
- [ ] ONNX model hot-reload
- [ ] Inference caching with content-addressed storage
- [ ] Batch prediction with adaptive batch sizing
- [ ] Model versioning and A/B testing
- [ ] Foundation model fine-tuning workflows

### Test Coverage Expansion
- [ ] oxigdal-node: 5 → 50+ tests
- [ ] oxigdal (umbrella): 8 → 50+ tests
- [ ] oxigdal-jupyter: 33 → 60+ tests
- [ ] oxigdal-services: 34 → 60+ tests
- [ ] oxigdal-metadata: 38 → 60+ tests
- [ ] Target: 10,000+ total tests

### Format Driver Improvements
- [ ] GeoTIFF: JPEG-in-TIFF decompression, LERC codec
- [ ] GeoParquet: nested geometry encoding, partitioned datasets
- [ ] Zarr v3: full sharding codec with partial chunk reads
- [ ] GRIB2: template-based product definition expansion
- [ ] NetCDF: CF conventions v1.11 full compliance

### API Ergonomics
- [ ] `oxigdal::open()` universal format detection
- [ ] Builder pattern for all readers/writers
- [ ] Streaming iterator API for large datasets
- [ ] Unified error context with source file/line

---

## v0.3.0 — Target: Q2 2026

### Streaming v2
- [ ] Backpressure-aware stream processing with credit-based flow control
- [ ] Session window improvements with gap detection
- [ ] Exactly-once semantics for Kafka/Kinesis/Pub/Sub
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
- [ ] GeoPackage (Pure Rust SQLite reader)
- [ ] MBTiles (vector tile archives)
- [ ] Cloud Optimized Point Cloud (COPC)
- [ ] PMTiles (single-file tile archive)

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
- [ ] Kubernetes Helm chart for oxigdal-server
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
- [ ] Replace unmaintained transitive deps: rustls-pemfile, sled/fxhash, evcxr/json, indicatif/number_prefix
- [ ] Track and patch security advisories within 48h
- [ ] Keep Arrow ecosystem at latest stable (currently 58)
- [ ] Keep all COOLJAPAN deps (oxiarc-*, scirs2-core, oxiblas, oxicode, OxiFFT) at latest

### Code Quality
- [ ] Maintain 0 clippy warnings, 0 rustdoc warnings
- [ ] Maintain 0 unwrap() in production code
- [ ] Maintain all files < 2,000 lines
- [ ] Increase test count toward 10,000+ (currently 14,605)
- [ ] Property-based testing (proptest) for core algorithms
- [ ] Fuzzing (cargo-fuzz) for format parsers (GeoTIFF, JPEG2000, GRIB)

### Platform
- [ ] RISC-V support (no_std)
- [ ] Redox OS compatibility testing
- [ ] WASM Component Model (wasm32-wasip2) support
- [ ] Python 3.13+ free-threaded mode testing

---

*Last updated: 2026-05-22*

## Stubs to implement (added 2026-06-12 by /cooljapan-stub-check)

- [ ] `oxigdal-wasm`: `crates/oxigdal-wasm/src/tests/functions_3.rs:542` — implement full Huffman codec for wasm decompression path (or wire oxiarc equivalent)
  - Priority: P2 | Scope: medium | Hint: oxiarc
- [ ] `oxigdal-wasm`: `crates/oxigdal-wasm/src/tests/functions_4.rs:249` — implement decompression roundtrip test
  - Priority: P2 | Scope: small | Hint: oxiarc
- [ ] `oxigdal-python`: `crates/oxigdal-python/src/numpy.rs:586` — implement proper complex dtype mapping when pyo3 supports it
  - Priority: P2 | Scope: small | Hint: none
- [ ] `oxigdal-algorithms`: `crates/oxigdal-algorithms/tests/texture_analysis_test.rs:212` — investigate GLCM computation on uniform rasters (empty matrix bug) and re-enable ignored test
  - Priority: P2 | Scope: small | Hint: none
- [ ] `oxigdal-workflow`: `crates/oxigdal-workflow/src/integrations/temporal.rs:27` — replace generated activity-logic TODO placeholder with real Temporal workflow activity bodies
  - Priority: P2 | Scope: medium | Hint: none
- [ ] `oxigdal-drivers/jpeg2000`: `crates/oxigdal-drivers/jpeg2000/src/lib.rs:154` — implement JPEG2000 driver (tracked as TODO in module doc)
  - Priority: P2 | Scope: large | Hint: none

## Policy follow-ups (added 2026-06-13 by /policy-check)

### #1 No-unwrap purge — production code only (tests on infallible paths are policy-allowed)
- [x] **oxigdal**: 0 production `.unwrap()` (clean)
  - Verified: 36 src/ files scanned; all `.unwrap()` calls are inside `#[cfg(test)]` blocks.
    Cross-confirmed by checking every `.rs` file for unwraps before the first `#[cfg(test)]`
    annotation — result: zero hits. `oxigdal` is fully policy-compliant on #1.

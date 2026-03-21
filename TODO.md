# OxiGDAL TODO

> Version: 0.1.3 (2026-03-21) | 76 crates | 10,939 tests | ~540K Rust SLoC

---

## v0.1.3 — Current Release (2026-03-21) [COMPLETE]

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
- [ ] EPSG expansion to 500+ definitions
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
- [ ] Increase test count toward 10,000+ (currently 10,935)
- [ ] Property-based testing (proptest) for core algorithms
- [ ] Fuzzing (cargo-fuzz) for format parsers (GeoTIFF, JPEG2000, GRIB)

### Platform
- [ ] RISC-V support (no_std)
- [ ] Redox OS compatibility testing
- [ ] WASM Component Model (wasm32-wasip2) support
- [ ] Python 3.13+ free-threaded mode testing

---

*Last updated: 2026-03-21*

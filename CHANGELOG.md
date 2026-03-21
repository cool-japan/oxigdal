# Changelog

All notable changes to OxiGDAL will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

- **Homepage**: <https://github.com/cool-japan/oxigdal>
- **Documentation**: <https://docs.rs/oxigdal>
- **Issue Tracker**: <https://github.com/cool-japan/oxigdal/issues>

[Unreleased]: https://github.com/cool-japan/oxigdal/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/cool-japan/oxigdal/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/cool-japan/oxigdal/releases/tag/v0.1.0

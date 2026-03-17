# OxiGDAL

**Pure Rust Geospatial Data Abstraction Library — Production-Grade GDAL Alternative**

[![Crates.io](https://img.shields.io/crates/v/oxigdal.svg)](https://crates.io/crates/oxigdal)
[![Documentation](https://docs.rs/oxigdal/badge.svg)](https://docs.rs/oxigdal)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![COOLJAPAN](https://img.shields.io/badge/COOLJAPAN-Ecosystem-brightgreen.svg)](https://github.com/cool-japan)

OxiGDAL is a comprehensive, production-ready geospatial data abstraction library written in **100% Pure Rust** with zero C/C++/Fortran dependencies in default features. Released as **v0.1.2** on 2026-03-17, it delivers ~540,000 Rust SLoC across **76 workspace crates**, covering 15 geospatial format drivers, full CRS transformations, raster/vector algorithms, cloud-native I/O, GPU acceleration, enterprise security, and cross-platform bindings (Python, Node.js, WASM, iOS, Android).

## Project Statistics

| Metric | Value |
|--------|-------|
| **Version** | 0.1.2 (released 2026-03-17) |
| **Rust SLoC** | ~540,000 across 1,934 `.rs` files |
| **Total SLoC** | 565,681 (all languages) |
| **Workspace crates** | 76 |
| **Tests** | 10,935 passing (45 skipped), 0 failures |
| **Format drivers** | 15 (GeoTIFF/COG, GeoJSON, GeoParquet, Zarr, FlatGeobuf, Shapefile, NetCDF, HDF5, GRIB, JPEG2000, VRT, COPC/LAS, GeoPackage, MBTiles, PMTiles) |
| **EPSG definitions** | 211+ embedded (all UTM zones, national grids), O(1) lookup |
| **Map projections** | 20+ (UTM 1-60, Web Mercator, LCC, Albers, Polar Stereo, Japan Plane Rect, ...) |
| **Supported platforms** | Linux, macOS, Windows, WASM, iOS, Android, embedded (no_std) |
| **Estimated dev cost** | $20.97M equivalent (COCOMO) |

## Why OxiGDAL?

| | GDAL (C/C++) | OxiGDAL (Rust) |
|---|---|---|
| **Dependencies** | C/C++ toolchain, PROJ, GEOS, libcurl, ... | `cargo add oxigdal` |
| **Cross-compilation** | Complex per-target | Trivial (WASM, iOS, Android, embedded) |
| **Memory safety** | Manual management | Guaranteed by Rust |
| **Concurrency** | Thread-unsafe APIs | Fearless concurrency |
| **Binary size** | ~50MB+ monolith | Pay-for-what-you-use features |
| **WASM** | Not supported | < 1MB gzipped bundle |
| **Error handling** | C error codes | Rich typed `Result<T, OxiError>` |
| **Async I/O** | Blocking only | First-class async |

## Quick Start

```toml
[dependencies]
oxigdal = "0.1"  # GeoTIFF + GeoJSON + Shapefile by default

# Full feature set:
oxigdal = { version = "0.1", features = ["full"] }
```

```rust
use oxigdal::Dataset;

fn main() -> oxigdal::Result<()> {
    let dataset = Dataset::open("world.tif")?;
    println!("Format : {}", dataset.format());
    println!("Size   : {}x{}", dataset.width(), dataset.height());
    println!("CRS    : {}", dataset.crs().name());
    Ok(())
}
```

## Architecture

76 workspace crates organized into functional layers:

```
Core & Algorithms
  oxigdal                    Umbrella crate (unified API entry-point)
  oxigdal-core               Types, traits, async I/O, Arrow buffers, no_std core
  oxigdal-proj               Pure Rust PROJ: 20+ projections, 211+ EPSG, WKT2
  oxigdal-algorithms         SIMD raster/vector algorithms (AVX2, AVX-512, NEON)
  oxigdal-index              Spatial indexing (R-tree, grid, geometry validation/operations)
  oxigdal-qc                 Data validation, anomaly detection, quality scoring

Format Drivers (15 formats)
  geotiff      GeoTIFF/COG   BigTIFF, HTTP range, overviews, DEFLATE/LZW/ZSTD/JPEG
  geojson      GeoJSON       RFC 7946, streaming parser, GeoArrow zero-copy
  geoparquet   GeoParquet    Arrow native, spatial predicate pushdown, 10x faster
  zarr         Zarr v2/v3    Sharding, codec pipeline, consolidated metadata
  flatgeobuf   FlatGeobuf    Packed Hilbert R-tree, spatial filter during decode
  shapefile    Shapefile     SHP/SHX/DBF, full attribute table support
  netcdf       NetCDF        CF conventions, unlimited dims, group hierarchies
  hdf5         HDF5          Hierarchical, chunking, compression, attributes
  grib         GRIB1/2       Meteorological parameter/level tables
  jpeg2000     JPEG2000      Wavelet DWT, full EBCOT tier-1 decoder (MQ coder, 3-pass)
  vrt          VRT           Band math, source mosaicking, on-the-fly processing
  copc         COPC/LAS      Cloud Optimized Point Cloud (LAS 1.4, octree)
  gpkg         GeoPackage    SQLite-based, vector features + tiles
  mbtiles      MBTiles       Tile storage, TMS/XYZ schemes
  pmtiles      PMTiles v3    Hilbert curve, single-file tile archive
  geojson-s    GeoJSON (streaming)  Streaming GeoJSON parser/writer/filter

Cloud & Storage
  oxigdal-cloud              S3 / GCS / Azure Blob backends with HTTP range support
  oxigdal-cloud-enhanced     Multi-cloud orchestration, auto-tiering
  oxigdal-drivers-advanced   Multi-part S3, ADLS, GCS optimized reads
  oxigdal-compress           OxiArc compression: Deflate, LZ4, Zstd, BZip2, LZW
  oxigdal-cache-advanced     Multi-tier: in-memory LRU -> disk -> Redis
  oxigdal-rs3gw              Rust S3-compatible gateway

Domain Modules
  oxigdal-3d                 3D Tiles 1.0 (B3DM, I3DM, PNTS), glTF, Delaunay
  oxigdal-terrain            DEM, hydrology, viewshed, TRI/TPI, watershed
  oxigdal-temporal           Time-series datacube, change detection, gap filling
  oxigdal-analytics          Spatial stats, Getis-Ord Gi*, clustering, zonal ops
  oxigdal-sensors            IoT sensor ingestion, calibration, SOS
  oxigdal-metadata           ISO 19115:2014, ISO 19139 XML, FGDC CSDGM
  oxigdal-stac               SpatioTemporal Asset Catalog 1.0.0 client
  oxigdal-query              SQL-like geospatial query engine with optimizer

Enterprise & Infrastructure
  oxigdal-server             OGC server: WMS 1.3.0, WFS 2.0.0
  oxigdal-gateway            API gateway: JWT, OAuth2, rate limiting
  oxigdal-security           AES-256-GCM, ChaCha20-Poly1305, Argon2id, RBAC/ABAC
  oxigdal-observability      Prometheus metrics, OpenTelemetry tracing, alerting
  oxigdal-services           WMS/WFS endpoints, health checks
  oxigdal-workflow           Workflow automation and scheduling
  oxigdal-distributed        Distributed partitioning and sharding
  oxigdal-cluster            Raft consensus-based cluster coordination
  oxigdal-ha                 High-availability failover and leader election
  oxigdal-postgis            PostGIS connector
  oxigdal-db-connectors      PostgreSQL, SQLite, DuckDB connectors

Streaming & Messaging
  oxigdal-streaming          Real-time stream processing
  oxigdal-kafka              Apache Kafka integration
  oxigdal-kinesis            AWS Kinesis integration
  oxigdal-pubsub             Google Pub/Sub integration
  oxigdal-mqtt               MQTT IoT sensor messaging
  oxigdal-websocket          WebSocket real-time updates
  oxigdal-ws                 WS/WSS server
  oxigdal-etl                ETL pipeline engine
  oxigdal-sync               CRDT-based offline sync (OR-Set, Merkle tree, vector clocks)

Platform Bindings
  oxigdal-wasm               WebAssembly: WasmCogViewer JS/TS API, < 1MB gzipped
  oxigdal-pwa                Progressive Web App: Service Worker, offline-first
  oxigdal-offline            Offline-first sync, operation queue, delta sync
  oxigdal-node               Node.js N-API bindings (napi-rs, CJS + ESM)
  oxigdal-python             Python bindings (PyO3/Maturin, NumPy, manylinux wheels)
  oxigdal-jupyter            Jupyter kernel (evcxr + plotters rich display)
  oxigdal-mobile             iOS (Swift FFI) and Android (Kotlin/JNI)
  oxigdal-mobile-enhanced    Battery/network-aware mobile scheduling
  oxigdal-embedded           no_std for microcontrollers (heapless, embedded-hal)
  oxigdal-noalloc            no_std geospatial primitives (zero heap allocation)
  oxigdal-edge               Edge computing, streaming sensor ingestion, local DB

GPU & ML
  oxigdal-gpu                GPU acceleration (wgpu compute shaders)
  oxigdal-gpu-advanced       Advanced GPU kernels
  oxigdal-ml                 ML pipeline integration
  oxigdal-ml-foundation      Foundation model support

Tooling
  oxigdal-cli                CLI: info, convert, dem, rasterize, warp (Clap)
  oxigdal-dev-tools          File watching, progress bars (indicatif), diff utils
  oxigdal-bench              Criterion benchmarks with pprof flamegraph profiling
  oxigdal-examples           Runnable examples
```

## Format Support

| Format | Read | Write | Async | Cloud | Notes |
|--------|------|-------|-------|-------|-------|
| GeoTIFF / COG | yes | yes | yes | yes | BigTIFF, overviews, HTTP range |
| GeoJSON | yes | yes | yes | yes | RFC 7946, streaming, GeoArrow |
| GeoParquet | yes | yes | yes | yes | Arrow-native, 10x faster than GeoPandas |
| Zarr v2/v3 | yes | yes | yes | yes | Sharding, codec pipeline |
| FlatGeobuf | yes | yes | yes | yes | Spatial filter during decode |
| Shapefile | yes | yes | — | — | SHP/SHX/DBF |
| NetCDF | yes | partial | — | — | CF conventions, unlimited dims |
| HDF5 | yes | partial | — | — | Chunking, groups, attributes |
| GRIB1/GRIB2 | yes | — | — | — | Meteorological parameter tables |
| JPEG2000 | yes | — | — | — | Wavelet DWT, tier-1 |
| VRT | yes | yes | — | — | Band math, mosaic |
| COPC/LAS | yes | — | — | — | Point cloud, octree spatial index |
| GeoPackage | yes | — | — | — | SQLite-based, vector features + tiles |
| MBTiles | yes | yes | — | — | Tile storage, TMS/XYZ |
| PMTiles v3 | yes | yes | — | — | Hilbert curve, single-file archive |

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `geotiff` | yes | GeoTIFF / Cloud Optimized GeoTIFF |
| `geojson` | yes | GeoJSON (RFC 7946) |
| `shapefile` | yes | ESRI Shapefile |
| `full` | no | All 15 format drivers |
| `proj` | no | CRS transformations (20+ projections, 211+ EPSG) |
| `algorithms` | no | SIMD raster/vector algorithms |
| `cloud` | no | S3, GCS, Azure Blob storage |
| `async` | no | Async I/O traits |
| `arrow` | no | Apache Arrow zero-copy |
| `gpu` | no | GPU acceleration (wgpu) |
| `ml` | no | Machine learning pipeline |
| `server` | no | OGC WMS/WFS tile server |
| `security` | no | AES-256-GCM, TLS 1.3, RBAC |
| `distributed` | no | Distributed cluster support |
| `streaming` | no | Real-time stream processing |
| `gpkg` | no | GeoPackage format support |
| `pmtiles` | no | PMTiles v3 format support |
| `mbtiles` | no | MBTiles format support |
| `copc` | no | COPC/LAS point cloud |
| `index` | no | Spatial indexing and geometry operations |
| `services` | no | OGC services (WMS/WFS/WCS/WPS) |

## Usage Examples

### GeoTIFF / COG

```rust
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_core::io::FileDataSource;

let source = FileDataSource::open("elevation.tif")?;
let reader = GeoTiffReader::open(source)?;
println!("Size  : {}x{}", reader.width(), reader.height());
println!("Bands : {}", reader.band_count());

// COG tile access (HTTP range requests supported transparently)
let tile = reader.read_tile(0, 0, 0)?;
```

### CRS Transformation

```rust
use oxigdal_proj::{Crs, Transformer};

let wgs84  = Crs::from_epsg(4326)?;
let utm54n = Crs::from_epsg(32654)?;   // UTM Zone 54N (Japan)
let tf     = Transformer::new(&wgs84, &utm54n)?;

// SIMD-vectorized batch: < 10ms for 1M points
let (easting, northing) = tf.transform(139.7671, 35.6812)?;
```

### Raster Algorithms

```rust
use oxigdal_algorithms::raster::{hillshade, reproject, ResamplingMethod};

// SIMD hillshade (AVX2 / NEON auto-selected at runtime)
let shaded = hillshade(&dem, 315.0, 45.0)?;
let warped = reproject(&src, &target_crs, ResamplingMethod::Bilinear)?;
```

### GeoParquet (Arrow)

```rust
use oxigdal_geoparquet::GeoParquetReader;

let reader   = GeoParquetReader::open("buildings.parquet")?;
let filter   = BoundingBox::new(135.0, 34.0, 137.0, 36.0)?;
let features = reader.read_with_bbox_filter(&filter)?;
```

### Python Bindings

```python
import oxigdal

ds  = oxigdal.open("satellite.tif")
arr = ds.read(1)           # returns numpy ndarray
gdf = oxigdal.read_geoparquet("buildings.parquet")  # Arrow-native
```

### WebAssembly

```javascript
import init, { WasmCogViewer } from '@cooljapan/oxigdal';
await init();

const viewer = new WasmCogViewer();
await viewer.open('https://example.com/cog.tif');

const imageData = await viewer.read_tile_as_image_data(0, 0, 0);
ctx.putImageData(imageData, 0, 0);
```

### CLI

```bash
oxigdal info world.tif
oxigdal convert input.shp output.fgb
oxigdal dem --hillshade elevation.tif hillshade.tif
oxigdal warp --t_srs EPSG:32654 input.tif output.tif
```

## Enterprise Features

### Security (`oxigdal-security`, `oxigdal-gateway`)

- Encryption at rest: AES-256-GCM and ChaCha20-Poly1305
- Password hashing: Argon2id
- Transport: TLS 1.3 via `rustls` (no OpenSSL)
- Authentication: JWT, OAuth2
- Authorization: RBAC and ABAC
- Audit logging: SOC2 and GDPR-ready
- Message integrity: HMAC-SHA256
- All crypto: pure Rust (`ring`, `rustls`, `aes-gcm`, `chacha20poly1305`, `argon2`)

### High Availability (`oxigdal-ha`, `oxigdal-cluster`)

- Raft consensus-based cluster coordination
- Automatic failover and leader election
- Distributed partitioning and sharding (`oxigdal-distributed`)
- Multi-tier cache: in-memory LRU -> on-disk -> Redis (`oxigdal-cache-advanced`)
- CRDT-based offline sync with Merkle tree verification (`oxigdal-sync`)

### Streaming & Messaging

| Crate | Integration |
|-------|-------------|
| `oxigdal-streaming` | Real-time stream processing |
| `oxigdal-kafka` | Apache Kafka |
| `oxigdal-kinesis` | AWS Kinesis |
| `oxigdal-pubsub` | Google Pub/Sub |
| `oxigdal-mqtt` | MQTT / IoT |
| `oxigdal-websocket` | WebSocket real-time |

### OGC Services (`oxigdal-server`)

- WMS 1.3.0 tile server
- WFS 2.0.0 feature service
- API gateway with JWT auth and rate limiting

## Performance

| Operation | Result |
|-----------|--------|
| COG tile access (local SSD) | < 10ms |
| COG tile access (S3/GCS) | < 100ms |
| GeoTIFF metadata reading | < 5ms |
| GeoParquet vs GeoPandas | 10x faster |
| PROJ batch transform (1M pts) | < 10ms |
| Docker image size | < 50MB (vs 1GB+ for GDAL) |
| WASM bundle (gzipped) | < 1MB |

## Platform Support

| Platform | Status | Notes |
|----------|--------|-------|
| Linux x86_64 | Production | AVX2 / AVX-512 SIMD |
| Linux aarch64 | Production | NEON SIMD |
| macOS Apple Silicon | Production | NEON SIMD |
| macOS x86_64 | Production | AVX2 SIMD |
| Windows x86_64 | Production | |
| WebAssembly (wasm32) | Production | < 1MB bundle, IndexedDB |
| iOS arm64 | Production | Swift FFI |
| Android arm64 | Production | Kotlin/JNI |
| Embedded no_std | Stable | heapless, embedded-hal |
| Python (PyPI) | Production | manylinux2014, macOS, Windows wheels |
| Node.js 16+ | Production | napi-rs, CommonJS + ESM |

## COOLJAPAN Ecosystem Compliance

| Policy | Status |
|--------|--------|
| Pure Rust (default features) | 100% Rust; C/Fortran behind feature flags |
| No `unwrap()` | `clippy::unwrap_used = "deny"` (0 in production code; 2 in non-compiled doc comments) |
| Workspace versions | All via `*.workspace = true` |
| Latest crates | All deps at latest crates.io versions |
| No OpenBLAS | Uses `oxiblas` |
| No `bincode` | Uses `oxicode` |
| No `zip` crate | Uses `oxiarc-*` ecosystem |
| No `rustfft` | Uses `OxiFFT` |

## Roadmap

| Release | Target | Focus |
|---------|--------|-------|
| **v0.1.0** | 2026-02-22 (released) | Independence: 68 crates, 11 drivers, ~500K SLoC, full enterprise stack |
| **v0.1.1** | 2026-03-11 (released) | EBCOT tier-1 decoder, EPSG expansion (211+), floating-point predictor, Pure Rust compression, CLI commands, 69 crates, 7,486 tests |
| **v0.1.2** | 2026-03-17 (released) | Wave 7: ogc_features/epsg refactoring, PMTiles writer, geometry validation/operations, umbrella crate integration, 76 crates, 10,935 tests |
| **v0.2.0** | Q2 2026 | 100+ projections, GPU expansion, advanced ML pipelines, JPEG2000 tier-2 |
| **v0.3.0** | Q3 2026 | Streaming v2, cloud-native tile server v2, extended STAC support |
| **v1.0.0** | Q4 2026 | LTS commitment, enterprise compliance certifications |

## Development

```bash
cargo build --all-features
cargo nextest run --all-features
cargo clippy --all-features -- -D warnings
tokei .
```

See `crates/oxigdal-examples/src/` for runnable examples.

## Documentation

| Resource | Location |
|---------|----------|
| API Reference | https://docs.rs/oxigdal |
| Getting Started | [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md) |
| Architecture | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| Drivers | [docs/DRIVERS.md](docs/DRIVERS.md) |
| Algorithms | [docs/ALGORITHMS.md](docs/ALGORITHMS.md) |
| GDAL Migration | [docs/MIGRATION.md](docs/MIGRATION.md) |
| CHANGELOG | [CHANGELOG.md](CHANGELOG.md) |

## Contributing

Follow [COOLJAPAN policies](docs/BEST_PRACTICES.md):

1. No `unwrap()` or `expect()` in production code
2. Files must stay under 2,000 lines (use `splitrs` for refactoring)
3. All dependencies via workspace (`*.workspace = true`)
4. Run `cargo clippy --all-features -- -D warnings` before submitting
5. Use `cargo nextest run --all-features` for testing

## License

Licensed under the Apache License, Version 2.0 ([LICENSE](LICENSE)).

## Acknowledgments

- **GDAL Project** — original inspiration and reference implementation
- **GeoRust Community** — ecosystem collaboration
- **PROJ** — CRS reference and test suite
- **Specifications**: GeoTIFF, COG, OGC (WMS/WFS), STAC, ISO 19115, RFC 7946

---

**Made with love by [COOLJAPAN OU (Team Kitasan)](https://github.com/cool-japan)**  
**Pure Rust · Cloud Native · WebAssembly · Production Enterprise**

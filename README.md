# OxiGeo

**Pure Rust Geospatial Data Abstraction Library — Production-Grade GDAL Alternative**

[![Crates.io](https://img.shields.io/crates/v/oxigeo.svg)](https://crates.io/crates/oxigeo)
[![Documentation](https://docs.rs/oxigeo/badge.svg)](https://docs.rs/oxigeo)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![COOLJAPAN](https://img.shields.io/badge/COOLJAPAN-Ecosystem-brightgreen.svg)](https://github.com/cool-japan)

> **Note:** **OxiGeo** is the new name of **OxiGDAL**. v0.1.7 was the final release under the OxiGDAL name; development continues as OxiGeo from v0.2.0 with an otherwise identical codebase. This project is an independent reimplementation and is not affiliated with the GDAL project.

[![OxiGeo GeoSentinel — Sentinel-2 change detection running entirely in the browser in Pure-Rust WebAssembly: NDVI-drop polygons over the Lahaina wildfire burn scar](docs/media/geosentinel-hero.png)](https://cooljapan.tech/geosentinel/)

**GeoSentinel**: watch any place on Earth for change — and tell no one where you're looking. It searches the public Earth Search STAC API for a cloud-filtered Sentinel-2 scene pair, streams only the needed COG windows via HTTP range requests, and runs the whole change-detection pipeline — NDVI difference, thresholding, polygonization, geodesic areas, GeoJSON export — 100% client-side in Pure-Rust WebAssembly; your area of interest never leaves your machine. **[Try it live](https://cooljapan.tech/geosentinel/)** — one of **four** hosted [demos](#demos) below, alongside [GeoLab](#geolab--terrain-analysis-on-streamed-cogs), [GeoVault](#geovault--sovereign-clean-room-workstation), and [GeoParquet Live](#geoparquet-live--query-59-gb-with-no-database).

OxiGeo is a comprehensive, production-ready geospatial data abstraction library written in **100% Pure Rust** with zero C/C++/Fortran dependencies in default features. The current release is **v0.2.2**, a correctness campaign centered on [issue #14](https://github.com/cool-japan/oxigeo/issues/14) — `Dataset::read_band` silently returning the whole pixel-interleaved image instead of one band — root-caused and fixed at the GeoTIFF driver level, with the same defect pattern independently found and fixed across a dozen downstream crates. It builds on **v0.2.1**'s production-hardening pass (2026-07-28) and **v0.2.0**, published to crates.io on 2026-07-20 as the first release under the OxiGeo name — a rename-only release, functionally identical to v0.1.7, which was published the same day as the final release under the OxiGDAL name (v0.1.6 released 2026-06-15). The library delivers ~791K Rust SLoC across **75 workspace crates**, covering 16 geospatial format drivers exposed through `oxigeo::Dataset::open()` (plus separate KML/KMZ and TopoJSON format support in dedicated crates, see [Format Drivers](#architecture)), full CRS transformations, raster/vector algorithms, cloud-native I/O, GPU acceleration, enterprise security, and cross-platform bindings (Python, Node.js, WASM, iOS, Android).

## Project Statistics

| Metric | Value |
|--------|-------|
| **Version** | 0.2.2 (2026-07-30, issue #14 correctness campaign); v0.2.1 released 2026-07-28 (production-hardening); v0.2.0 published 2026-07-20 (first OxiGeo release, rename-only); v0.1.7 published 2026-07-20 (final OxiGDAL release); v0.1.6 released 2026-06-15 |
| **Rust SLoC** | ~791K across 2,501 `.rs` files (via `tokei`) |
| **Total SLoC** | ~819K (all languages, via `tokei`) |
| **Workspace crates** | 75 |
| **Tests** | 18,133 passing (101 skipped), 0 failures (`--all-features`); 16,684 passing (80 skipped) on default features; 402 doc tests passing |
| **Format drivers** | 16, exposed through `oxigeo::Dataset::open()` (GeoTIFF/COG, GeoJSON, Shapefile, GeoParquet, NetCDF, HDF5, Zarr, GRIB, FlatGeobuf, JPEG2000, VRT, GeoPackage, PMTiles, MBTiles, COPC, LAS/LAZ) — plus KML/KMZ (`oxigeo-drivers-advanced`) and TopoJSON writer (`oxigeo-geojson`), which ship as separate, non-`Dataset`-registry format support |
| **EPSG definitions** | 211+ embedded (all UTM zones, national grids), O(1) lookup |
| **Map projections** | 20+ (UTM 1-60, Web Mercator, LCC, Albers, Polar Stereo, Japan Plane Rect, ...) |
| **Supported platforms** | Linux, macOS, Windows, WASM, iOS, Android, embedded (no_std) |
| **Estimated dev cost** | $31.68M equivalent (COCOMO) |

## Why OxiGeo?

| | GDAL (C/C++) | OxiGeo (Rust) |
|---|---|---|
| **Dependencies** | C/C++ toolchain, PROJ, GEOS, libcurl, ... | `cargo add oxigeo` |
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
oxigeo = "0.2"  # GeoTIFF + GeoJSON + Shapefile by default

# Full feature set:
oxigeo = { version = "0.2", features = ["full"] }
```

```rust
use oxigeo::Dataset;

fn main() -> oxigeo::Result<()> {
    let dataset = Dataset::open("world.tif")?;
    println!("Format : {}", dataset.format());
    println!("Size   : {}x{}", dataset.width(), dataset.height());
    println!("CRS    : {}", dataset.crs().unwrap_or("unknown"));
    Ok(())
}
```

## Demos

Four live, hosted demos — each one runs 100% client-side: Pure-Rust WebAssembly
in your browser tab, no server-side processing, no accounts, no telemetry.

| Demo | One-liner | Live |
|------|-----------|------|
| **GeoLab** | Stream, decode, and terrain-analyze Cloud-Optimized GeoTIFFs | [cooljapan.tech/geolab](https://cooljapan.tech/geolab/) |
| **GeoSentinel** | Watch any place on Earth for change — and tell no one where you're looking | [cooljapan.tech/geosentinel](https://cooljapan.tech/geosentinel/) |
| **GeoVault** | A clean-room workstation that seals a signed, tamper-evident ledger of everything it did | [cooljapan.tech/geovault](https://cooljapan.tech/geovault/) |
| **GeoParquet Live** | Query a dataset bigger than your laptop, over the network, with no database | [cooljapan.tech/geoparquet](https://cooljapan.tech/geoparquet/) |

### GeoLab — terrain analysis on streamed COGs

The [GeoLab](https://cooljapan.tech/geolab/) viewer is a Pure-Rust WebAssembly
build of the raster pipeline — GeoTIFF decode, `combined_hillshade`, and colormap
rendering all run in the browser tab, not on a server.

![GeoLab terrain interaction: dragging the sun-azimuth and exaggeration sliders re-renders the San Francisco DEM's hillshade in real time, entirely client-side.](docs/media/geolab-terrain.gif)

Hosted, nothing to install: **[cooljapan.tech/geolab](https://cooljapan.tech/geolab/)**

Or run it locally (builds the WASM module, then serves the static demo):

```sh
cd crates/oxigeo-wasm
wasm-pack build --scope cooljapan --target web --out-dir pkg --release
cd ../../demo/cog-viewer
python3 -m http.server 8080
# open http://localhost:8080
```

#### Native-render gallery

The same algorithms, invoked directly from Rust (no browser, no WASM) against the
San Francisco / Marin Headlands SRTM DEM:

<table>
<tr>
<td align="center">
<img src="docs/media/geolab-render-terrain.jpg" width="380" alt="Multidirectional hillshade multiply-blended with a terrain colormap over the San Francisco DEM"><br>
<sub><code>combined_hillshade</code> × <code>Colormap::Terrain</code></sub>
</td>
<td align="center">
<img src="docs/media/geolab-render-slope.jpg" width="380" alt="Slope-angle raster over the San Francisco DEM, spectral colormap"><br>
<sub><code>slope()</code> × <code>Colormap::Spectral</code></sub>
</td>
</tr>
<tr>
<td align="center">
<img src="docs/media/geolab-render-aspect.jpg" width="380" alt="Aspect (slope-facing direction) raster over the San Francisco DEM, jet colormap, flat areas masked gray"><br>
<sub><code>aspect()</code> masked by <code>slope()</code> × <code>Colormap::Jet</code></sub>
</td>
<td align="center">
<img src="docs/media/geolab-render-elevation.jpg" width="380" alt="Raw elevation raster over the San Francisco DEM, viridis colormap"><br>
<sub>elevation × <code>Colormap::Viridis</code></sub>
</td>
</tr>
</table>

> Every image on this page — the hero screenshots, the interaction GIFs, the
> demo galleries, and the four native renders above — is a real capture or
> render produced by this repository's own code, not a mockup. Reproduce the
> native gallery yourself with:
> `cargo run -p oxigeo-server --example render_hero --release -- --mode all`

### GeoSentinel — in-browser Sentinel-2 change detection

[![OxiGeo GeoSentinel — Sentinel-2 change detection running entirely in the browser: NDVI-drop polygons over the Lahaina wildfire burn scar](docs/media/geosentinel-hero.png)](https://cooljapan.tech/geosentinel/)

**Watch any place on Earth for change — and tell no one where you're looking.**
GeoSentinel searches the public Earth Search STAC API for a cloud-filtered
Sentinel-2 L2A scene pair, streams only the needed COG windows (red + NIR
bands) via HTTP range requests, and runs the whole change-detection pipeline —
NDVI difference, fixed or Otsu thresholding, polygonization, Karney geodesic
areas, GeoJSON export — inside WebAssembly. **[Try it live](https://cooljapan.tech/geosentinel/)**

![GeoSentinel demo: drawing an area of interest over Lahaina, running change detection, then crossfading the before/after true-color scenes with the detected burn-scar polygons overlaid.](docs/media/geosentinel-demo.gif)

Hosted, nothing to install: **[cooljapan.tech/geosentinel](https://cooljapan.tech/geosentinel/)**

Or run it locally (builds the WASM package if missing, then serves the demo):

```sh
cd demo/geosentinel
./run.sh
# open http://localhost:8080
```

<table>
<tr>
<td align="center">
<img src="docs/media/geosentinel-before.jpg" width="380" alt="Sentinel-2 true-color scene of Lahaina before the August 2023 wildfire"><br>
<sub>Scene A — true color (before)</sub>
</td>
<td align="center">
<img src="docs/media/geosentinel-after.jpg" width="380" alt="Sentinel-2 true-color scene of Lahaina after the August 2023 wildfire"><br>
<sub>Scene B — true color (after)</sub>
</td>
</tr>
<tr>
<td align="center">
<img src="docs/media/geosentinel-diff.jpg" width="380" alt="NDVI-drop heatmap between the two Lahaina scenes, red marking vegetation loss"><br>
<sub>NDVI-drop heatmap (red = vegetation loss)</sub>
</td>
<td align="center">
<img src="docs/media/geosentinel-polygons.jpg" width="380" alt="Vectorized change polygons with per-polygon hectare areas over Lahaina"><br>
<sub>Change polygons + geodesic hectares (GeoJSON export)</sub>
</td>
</tr>
</table>

> **Honest notes** — the analysis (NDVI, thresholding, polygonization,
> geodesic areas) runs locally in your tab; the Sentinel-2 imagery is streamed
> directly from the AWS open-data bucket (`sentinel-cogs` S3, found via the
> public Earth Search STAC API); your location and area of interest are never
> sent to any backend of ours — there isn't one. Verified example: the Lahaina
> wildfire preset detects **713 ha** of burn scar (≈880 ha ground truth) from
> **9.0 MB** of streamed imagery.

### GeoVault — sovereign clean-room workstation

[![OxiGeo GeoVault — sovereign analysis workstation with a live tamper-evident session ledger and seal-session attestation](docs/media/geovault-hero.png)](https://cooljapan.tech/geovault/)

**Analyze sensitive terrain data in a browser clean-room that can prove its
session log afterwards.** Every operation is appended to a blake3 hash chain,
rolled up into a Merkle root, and sealed with an Ed25519 signature — producing
a downloadable attestation that an independent verifier page (or a native Rust
example) re-checks from the JSON alone, while a strict Content-Security-Policy
forbids every external connection during the session.
**[Try it live](https://cooljapan.tech/geovault/)**

![GeoVault demo: loading the synthetic Site K-7 DEM, running hillshade and anomaly detection while the session ledger grows, then sealing the session and verifying the downloaded attestation.](docs/media/geovault-demo.gif)

Hosted, nothing to install: **[cooljapan.tech/geovault](https://cooljapan.tech/geovault/)**

Or run it locally (shares the GeoLab/GeoSentinel WASM package):

```sh
wasm-pack build crates/oxigeo-wasm --target web --out-dir pkg   # once, from repo root
cd demo/geovault
python3 -m http.server 8080
# open http://localhost:8080
```

<table>
<tr>
<td align="center">
<img src="docs/media/geovault-workstation.jpg" width="380" alt="GeoVault workstation: hillshaded Site K-7 DEM with the session ledger recording each operation"><br>
<sub>Workstation — every action lands in the session ledger</sub>
</td>
<td align="center">
<img src="docs/media/geovault-anomaly.jpg" width="380" alt="Anomaly detection overlay highlighting the planted excavation pit in the synthetic DEM"><br>
<sub>Anomaly detection (Z-score / IQR / modified-Z)</sub>
</td>
</tr>
<tr>
<td align="center">
<img src="docs/media/geovault-seal.jpg" width="380" alt="Seal-session modal showing Merkle root, public key, and Ed25519 signature with attestation download"><br>
<sub>Seal session — Merkle root + Ed25519 signature</sub>
</td>
<td align="center">
<img src="docs/media/geovault-verify.jpg" width="380" alt="Independent verify page re-checking chain, Merkle root, and signature of the attestation"><br>
<sub>Independent verifier — chain ✓ · root ✓ · signature ✓</sub>
</td>
</tr>
</table>

> **Trust model, stated honestly** — the attestation cryptographically proves
> that the recorded operation log is complete and unaltered since sealing
> (blake3 chain → Merkle root → Ed25519 seal); the zero-egress claim is
> **enforced** by a browser Content-Security-Policy and **observed** by
> in-page fetch/XHR/beacon hooks — it is not mathematically proven, because no
> browser page can prove what other software on the machine did. Log
> integrity: proven. No-egress: enforced and observed.

### GeoParquet Live — query 5.9 GB with no database

[![OxiGeo GeoParquet Live — bounding-box and SQL queries against a 5.9 GB remote GeoParquet, pruned to a handful of row groups in the browser](docs/media/geoparquet-hero.png)](https://cooljapan.tech/geoparquet/)

**Query a dataset bigger than your laptop, over the network, with no
database.** The browser points at the VIDA Japan building-footprints
GeoParquet — **5.9 GB, 47.66 million rows, 9,533 row groups** — and answers
bounding-box + attribute queries by downloading only the byte ranges that
survive metadata pruning: the Shinjuku preset prunes 9,533 row groups down to
**7 survivors**, fetches **4.7 MB** of column chunks, and refines the exact
matches in **13 ms**. **[Try it live](https://cooljapan.tech/geoparquet/)**

![GeoParquet Live demo: dragging a query box over Tokyo while the plan preview updates, then running the query — the row-group strip lights up the surviving row groups and buildings render on the map.](docs/media/geoparquet-demo.gif)

Hosted, nothing to install: **[cooljapan.tech/geoparquet](https://cooljapan.tech/geoparquet/)**

Or run it locally (`serve.py` answers HTTP Range requests with `206`, which
stock `python3 -m http.server` does not):

```sh
cd demo/geoparquet
./build.sh              # wasm-pack build of crates/oxigeo-wasm-geoparquet (once)
python3 serve.py 8080
# open http://127.0.0.1:8080
```

<table>
<tr>
<td align="center">
<img src="docs/media/geoparquet-strip.jpg" width="380" alt="Row-group strip: 9,533 cells — grey pruned, amber plan survivors, green actually fetched"><br>
<sub>All 9,533 row groups — grey pruned · amber survivors · green fetched</sub>
</td>
<td align="center">
<img src="docs/media/geoparquet-results.jpg" width="380" alt="Query results: building footprints rendered on the map, colored by dataset confidence"><br>
<sub>Confidence-colored footprints on a Leaflet canvas</sub>
</td>
</tr>
<tr>
<td align="center" colspan="2">
<img src="docs/media/geoparquet-badges.jpg" width="380" alt="Honesty badges reading: dataset 5.9 GB, fetched a few MB, uploaded 0, server none"><br>
<sub><code>dataset: 5.9 GB · fetched: N MB · uploaded: 0 · server: none</code></sub>
</td>
</tr>
</table>

> **Honest notes** — the 17.8 MB Parquet footer is fetched once, on first open
> only (the browser Cache API serves it on later visits); snappy-compressed
> GeoParquet is supported via the pure-Rust `snap` codec, while zstd-compressed
> files are not — no pure-Rust parquet zstd path exists yet (documented
> limitation); `plan()` previews row groups / bytes / request count before a
> single data byte is fetched, and over-broad queries are refused rather than
> silently downloaded.

## Architecture

75 workspace crates organized into functional layers:

```
Core & Algorithms
  oxigeo                    Umbrella crate (unified API entry-point)
  oxigeo-core               Types, traits, async I/O, Arrow buffers, no_std core
  oxigeo-proj               Pure Rust PROJ: 20+ projections, 211+ EPSG, WKT2
  oxigeo-algorithms         SIMD raster/vector algorithms (AVX2, AVX-512, NEON)
  oxigeo-index              Spatial indexing (R-tree, grid, geometry validation/operations)
  oxigeo-qc                 Data validation, anomaly detection, quality scoring

Format Drivers (16 formats, all wired into `oxigeo::Dataset::open()` / `DatasetFormat`)
  geotiff      GeoTIFF/COG   BigTIFF, HTTP range, overviews, DEFLATE/LZW/ZSTD/JPEG
  geojson      GeoJSON       RFC 7946, streaming parser, GeoArrow zero-copy
  shapefile    Shapefile     SHP/SHX/DBF, full attribute table support
  geoparquet   GeoParquet    Arrow native, spatial predicate pushdown
  netcdf       NetCDF        CF conventions, unlimited dims, root group (pure-Rust oxinetcdf)
  hdf5         HDF5          Hierarchical, attributes, real read/write (pure-Rust oxih5)
  zarr         Zarr v2/v3    Sharding, codec pipeline, consolidated metadata
  grib         GRIB1/2       Meteorological parameter/level tables
  flatgeobuf   FlatGeobuf    Packed Hilbert R-tree, spatial filter during decode
  jpeg2000     JPEG2000      Wavelet DWT, full EBCOT tier-1 decoder (MQ coder, 3-pass)
  vrt          VRT           Band math, source mosaicking, on-the-fly processing
  gpkg         GeoPackage    SQLite-based, vector features + tiles
  pmtiles      PMTiles v3    Hilbert curve, single-file tile archive
  mbtiles      MBTiles       Tile storage, TMS/XYZ schemes
  copc         COPC          Cloud Optimized Point Cloud (LAS 1.4, octree)
  las          LAS/LAZ       Plain (non-COPC) LAS/LAZ point clouds

  Additional format support (dedicated crates, not part of the
  `Dataset::open()` driver registry above, so not counted in the 16):
  drivers-adv  KML/KMZ       Read + write (oxigeo-drivers-advanced)
  geojson-s    TopoJSON      Writer only: arcs, quantization (oxigeo-geojson streaming module)

Cloud & Storage
  oxigeo-cloud              S3 / GCS / Azure Blob backends with HTTP range support
  oxigeo-cloud-enhanced     Multi-cloud orchestration, auto-tiering
  oxigeo-drivers-advanced   Multi-part S3, ADLS, GCS optimized reads
  oxigeo-compress           OxiArc compression: Deflate, LZ4, Zstd, BZip2, LZW
  oxigeo-cache-advanced     Multi-tier: in-memory LRU -> disk -> Redis
  oxigeo-rs3gw              Rust S3-compatible gateway

Domain Modules
  oxigeo-3d                 3D Tiles 1.0 (B3DM, I3DM, PNTS), glTF, Delaunay
  oxigeo-terrain            DEM, hydrology, viewshed, TRI/TPI, watershed
  oxigeo-temporal           Time-series datacube, change detection, gap filling
  oxigeo-analytics          Spatial stats, Getis-Ord Gi*, clustering, zonal ops
  oxigeo-sensors            IoT sensor ingestion, calibration, SOS
  oxigeo-metadata           ISO 19115:2014, ISO 19139 XML, FGDC CSDGM
  oxigeo-stac               SpatioTemporal Asset Catalog 1.0.0 client
  oxigeo-query              SQL-like geospatial query engine with optimizer

Enterprise & Infrastructure
  oxigeo-server             OGC server: WMS 1.3.0, WFS 2.0.0
  oxigeo-gateway            API gateway: JWT, OAuth2, rate limiting
  oxigeo-security           AES-256-GCM, ChaCha20-Poly1305, Argon2id, RBAC/ABAC
  oxigeo-observability      Prometheus metrics, OpenTelemetry tracing, alerting
  oxigeo-services           WMS/WFS endpoints, health checks
  oxigeo-workflow           Workflow automation and scheduling
  oxigeo-distributed        Distributed partitioning and sharding
  oxigeo-cluster            Raft consensus-based cluster coordination
  oxigeo-ha                 High-availability failover and leader election
  oxigeo-postgis            PostGIS connector
  oxigeo-db-connectors      PostgreSQL, SQLite, DuckDB connectors

Streaming & Messaging
  oxigeo-streaming          Real-time stream processing
  oxigeo-kafka              Apache Kafka integration (RETIRED in 0.2.1)
  oxigeo-kinesis            AWS Kinesis integration
  oxigeo-pubsub             Google Pub/Sub integration
  oxigeo-mqtt               MQTT IoT sensor messaging
  oxigeo-websocket          WebSocket real-time updates
  oxigeo-ws                 WS/WSS server
  oxigeo-etl                ETL pipeline engine
  oxigeo-sync               CRDT-based offline sync (OR-Set, Merkle tree, vector clocks)

Platform Bindings
  oxigeo-wasm               WebAssembly: WasmCogViewer JS/TS API, < 1MB gzipped
  oxigeo-pwa                Progressive Web App: Service Worker, offline-first
  oxigeo-offline            Offline-first sync, operation queue, delta sync
  oxigeo-node               Node.js N-API bindings (napi-rs, CJS + ESM)
  oxigeo-python             Python bindings (PyO3/Maturin, NumPy, manylinux wheels)
  oxigeo-jupyter            Jupyter kernel (evcxr + plotters rich display)
  oxigeo-mobile             iOS (Swift FFI) and Android (Kotlin/JNI)
  oxigeo-mobile-enhanced    Battery/network-aware mobile scheduling
  oxigeo-embedded           no_std for microcontrollers (heapless, embedded-hal)
  oxigeo-noalloc            no_std geospatial primitives (zero heap allocation)
  oxigeo-edge               Edge computing, streaming sensor ingestion, local DB

GPU & ML
  oxigeo-gpu                GPU acceleration (wgpu compute shaders)
  oxigeo-gpu-advanced       Advanced GPU kernels
  oxigeo-ml                 ML pipeline integration
  oxigeo-ml-foundation      Foundation model support

Tooling
  oxigeo-cli                CLI: info, convert, dem, rasterize, warp (Clap)
  oxigeo-dev-tools          File watching, progress bars (indicatif), diff utils
  oxigeo-bench              Criterion benchmarks with pprof flamegraph profiling
  oxigeo-examples           Runnable examples
```

## Format Support

| Format | Read | Write | Async | Cloud | Notes |
|--------|------|-------|-------|-------|-------|
| GeoTIFF / COG | yes | yes | yes | yes | BigTIFF, overviews, HTTP range |
| GeoJSON | yes | yes | yes | yes | RFC 7946, streaming, GeoArrow |
| Shapefile | yes | yes | — | — | SHP/SHX/DBF |
| GeoParquet | yes | yes | yes | yes | Arrow-native, spatial predicate pushdown |
| NetCDF | yes | partial | — | — | Pure-Rust `oxinetcdf`; CF conventions, unlimited dims; reader surfaces the root group; `scale_factor`/`add_offset`/`_FillValue` exposed as attributes, not auto-applied |
| HDF5 | yes | partial | — | — | Pure-Rust `oxih5`; v0-superblock files read fully, v2/v3-superblock files open but currently yield an empty tree (best-effort); write produces real contiguous HDF5 (no chunking/compression on write) |
| Zarr v2/v3 | yes | yes | yes | yes | Sharding, codec pipeline |
| GRIB1/GRIB2 | yes | — | — | — | Meteorological parameter tables |
| FlatGeobuf | yes | yes | yes | yes | Spatial filter during decode |
| JPEG2000 | yes | — | — | — | Wavelet DWT, tier-1 |
| VRT | yes | yes | — | — | Band math, mosaic |
| GeoPackage | yes | partial | — | — | SQLite-based, vector features + tiles (write: point feature tables only, single-page B-tree) |
| PMTiles v3 | yes | yes | — | — | Hilbert curve, single-file archive |
| MBTiles | yes | yes | — | — | Tile storage, TMS/XYZ |
| COPC | yes | — | — | — | Cloud Optimized Point Cloud, octree spatial index |
| LAS/LAZ | yes | — | — | — | Plain (non-COPC) point cloud, read via the same reader as COPC |

Additional, separately-shipped format support not listed above (not part of
the `oxigeo::Dataset::open()` driver registry, so not counted in the 16):
KML/KMZ read+write (`oxigeo-drivers-advanced`) and a TopoJSON writer
(`oxigeo-geojson` streaming module).

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `geotiff` | yes | GeoTIFF / Cloud Optimized GeoTIFF |
| `geojson` | yes | GeoJSON (RFC 7946) |
| `shapefile` | yes | ESRI Shapefile |
| `full` | no | All 16 format drivers |
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
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_core::io::FileDataSource;

let source = FileDataSource::open("elevation.tif")?;
let reader = GeoTiffReader::open(source)?;
println!("Size  : {}x{}", reader.width(), reader.height());
println!("Bands : {}", reader.band_count());

// COG tile access (HTTP range requests supported transparently)
let tile = reader.read_tile(0, 0, 0)?;
```

### CRS Transformation

```rust
use oxigeo_proj::{Coordinate, Crs, Transformer};

let wgs84  = Crs::from_epsg(4326)?;
let utm54n = Crs::from_epsg(32654)?;   // UTM Zone 54N (Japan)
let tf     = Transformer::new(wgs84, utm54n)?;   // takes ownership of both CRS

let tokyo = Coordinate::from_lon_lat(139.7671, 35.6812);
let utm   = tf.transform(&tokyo)?;
println!("{:.2}, {:.2}", utm.x, utm.y);

// SIMD-vectorized batch transform (Transverse Mercator / Mercator / LCC
// projections get a dedicated SIMD kernel; other projections fall back to
// scalar per-point transformation)
let points = vec![tokyo; 1_000_000];
let batch  = tf.transform_batch(&points)?;

// Reuse a cached, thread-safe Transformer across many calls instead of
// rebuilding the PROJ pipeline every time:
use oxigeo_proj::TransformerCache;
let cache   = TransformerCache::new(16);
let cached  = cache.get_or_build(4326, 32654)?;
let utm2    = cached.transform(&tokyo)?;
```

### Raster Algorithms

```rust
use oxigeo_algorithms::raster::{hillshade, HillshadeParams};
use oxigeo_algorithms::{Resampler, ResamplingMethod};

// SIMD hillshade (AVX2 / NEON auto-selected at runtime)
let shaded = hillshade(&dem, HillshadeParams::standard())?;

// SIMD-accelerated resampling (nearest / bilinear / bicubic / lanczos)
let resized = Resampler::new(ResamplingMethod::Bilinear).resample(&dem, 512, 512)?;

// Full CRS reprojection combines a `Transformer` (oxigeo-proj, per-pixel
// coordinate mapping) with a `Resampler` — see `oxigeo-cli`'s `warp`
// command (crates/oxigeo-cli/src/commands/warp.rs) for the reference
// implementation, or invoke it directly: `oxigeo warp --t-srs EPSG:32654 in.tif out.tif`
```

### GeoParquet (Arrow)

```rust
use oxigeo_core::types::BoundingBox;
use oxigeo_geoparquet::GeoParquetReader;

let mut reader = GeoParquetReader::open("buildings.parquet")?;
let bbox       = BoundingBox::new(135.0, 34.0, 137.0, 36.0)?;

// Row-group pruning via the spatial index, then an exact per-row bbox check
let batches = reader.read_filtered_exact(bbox)?;
```

### Python Bindings

```python
import oxigeo

ds   = oxigeo.open("satellite.tif")     # mode="r" by default
data = ds.read_band(1)                   # returns a NumPy ndarray
meta = ds.get_metadata()
print(f"Size: {meta['width']}x{meta['height']}")

result = oxigeo.calc("A * 2", A=data)   # raster algebra
oxigeo.write("output.tif", result, metadata=meta)
```

### WebAssembly

```javascript
import init, { WasmCogViewer } from '@cooljapan/oxigeo';
await init();

const viewer = new WasmCogViewer();
await viewer.open('https://example.com/cog.tif');

const imageData = await viewer.read_tile_as_image_data(0, 0, 0);
ctx.putImageData(imageData, 0, 0);
```

### CLI

```bash
oxigeo info world.tif
oxigeo convert input.shp output.fgb
oxigeo dem hillshade elevation.tif hillshade.tif --azimuth 315 --altitude 45
oxigeo warp --t-srs EPSG:32654 input.tif output.tif
```

## Enterprise Features

### Security (`oxigeo-security`, `oxigeo-gateway`)

- Encryption at rest: AES-256-GCM and ChaCha20-Poly1305
- Password hashing: Argon2id
- Transport: TLS 1.3 via `rustls` (no OpenSSL)
- Authentication: JWT, OAuth2
- Authorization: RBAC and ABAC
- Audit logging: SOC2 and GDPR-ready
- Message integrity: HMAC-SHA256
- All crypto: pure Rust (`ring`, `rustls`, `aes-gcm`, `chacha20poly1305`, `argon2`)

### High Availability (`oxigeo-ha`, `oxigeo-cluster`)

- Raft consensus-based cluster coordination
- Automatic failover and leader election
- Distributed partitioning and sharding (`oxigeo-distributed`)
- Multi-tier cache: in-memory LRU -> on-disk -> Redis (`oxigeo-cache-advanced`)
- CRDT-based offline sync with Merkle tree verification (`oxigeo-sync`)

### Streaming & Messaging

| Crate | Integration |
|-------|-------------|
| `oxigeo-streaming` | Real-time stream processing |
| `oxigeo-kafka` | Apache Kafka — **RETIRED in 0.2.1**, no further releases ([why](#retired-crates)) |
| `oxigeo-kinesis` | AWS Kinesis |
| `oxigeo-pubsub` | Google Pub/Sub |
| `oxigeo-mqtt` | MQTT / IoT |
| `oxigeo-websocket` | WebSocket real-time |

### OGC Services (`oxigeo-server`)

- WMS 1.3.0 tile server
- WFS 2.0.0 feature service
- API gateway with JWT auth and rate limiting

## Retired Crates

| Crate | Retired | Status |
|-------|---------|--------|
| `oxigeo-kafka` | 0.2.1 | No further releases; crates.io 0.0.1 and 0.2.0 yanked |

**`oxigeo-kafka` (Apache Kafka integration) was retired in 0.2.1** and will receive no
further releases. It was the only crate in the workspace that made a C toolchain
mandatory — `rdkafka-sys` builds librdkafka through `cmake` — which stands against the
COOLJAPAN Pure Rust Policy, and it had no consumers anywhere inside the workspace.
With it removed, `cargo check --workspace --all-features` no longer needs `cmake`.
The `kafka` features of `oxigeo-etl` and `oxigeo-workflow` were removed at the same
time. For messaging, use `oxigeo-streaming`, `oxigeo-kinesis`, `oxigeo-pubsub`, or
`oxigeo-mqtt`; to talk to Kafka specifically, use a Kafka client directly in your own
code. Workflow definitions can still describe Kafka endpoints via the pure-Rust
`IntegrationType::Kafka` / `MessageQueueType::Kafka` metadata enums in
`oxigeo-workflow`.

## Performance

| Operation | Result |
|-----------|--------|
| COG tile access (local SSD) | < 10ms |
| COG tile access (S3/GCS) | < 100ms |
| GeoTIFF metadata reading | < 5ms |
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
| **v0.1.3** | 2026-03-21 (released) | wgpu 29 API fixes, libsqlite3-sys compat, macOS rpath fix, oxiarc-brotli 6-bug patch, 76 crates, 10,939 tests |
| **v0.1.4** | 2026-04-19 (released) | Wave 1 algorithms (Weiler-Atherton clipping, Karney geodesic, DE-9IM, marching squares), Wave 2 R-tree+SIMD+NoAlloc+PMTiles reader+COPC+GeoPackage B-tree, ort→oxionnx ML migration, pyo3 0.28, 12,064 tests |
| **v0.1.5** | 2026-05-22 (released) | oxigeo-gpu WGSL RayMarchUniforms layout fix eliminated 120s GPU test hang (Metal compute kernel), 78 crates, 14,605 tests |
| **v0.1.6** | 2026-06-15 (released) | Pure-Rust SQLite migration (rusqlite → oxisql-sqlite-compat), non-UTF-8 DBF encoding via encoding_rs, WKT→PROJ string conversion, W-TinyLFU + Count-Min Sketch cache eviction, HDF5 v2/v3 superblock, Delaunay triangulation, batch QC runner + GPKG/STAC/radiometric validators, Gaussian MLC sensor classification, terrain GLCM textures/TPI/geomorphons/cost-distance, Whittaker + Savitzky-Golay time-series smoothers, GPX/KML/TopoJSON vector formats, 14,605 tests |
| **v0.1.7** | 2026-07-20 (final release under the OxiGDAL name) | Production-hardening campaign: 233 verified defects fixed across 69 crates (GeoTIFF float-predictor silent-corruption fix, JPEG2000 MQ-decoder spec conformance + real Tier-2 packet/precinct decode, real FlatGeobuf FlatBuffers wire format, LERC2 bit-stuffed decoder, HDF5 ScaleOffset/N-Bit real filters, RBAC pattern-match bypass fix), 3 new hosted demos (GeoSentinel change detection, GeoVault attestation workstation, GeoParquet Live), security attestation module (blake3 chain → Merkle root → Ed25519 seal), multicloud S3/GCS/Azure `build_backend()` factory, pure-Rust ONNX export encoder, WFS-T/WCS real transactions, 76 crates, 16,909 tests |
| **v0.2.0** | 2026-07-20 (released) | Project renamed: OxiGDAL → OxiGeo. Rename-only release, functionally identical to v0.1.7; all 74 crates republished as `oxigeo`/`oxigeo-<name>` |
| **v0.2.1** | 2026-07-28 (released) | Production-hardening campaign: 342 defects found workspace-wide, 314 fixed across 38 crate lanes (~520 files) — GeoTIFF/JPEG2000/GRIB/HDF5/NetCDF correctness fixes, WFS-T fail-closed security fix, header-driven allocation DoS hardening; new axum-backed `oxigeo-gateway` serving layer (GraphQL + WebSocket + load-balanced reverse proxy); `oxih5`/`oxinetcdf` bumped to 0.2.2, `scirs2` to 0.6.4; 75 crates, 17,723 tests |
| **v0.2.2** | 2026-07-30 (released) | Issue #14 fix campaign: `Dataset::read_band` root-caused and fixed at the GeoTIFF driver level (new `band_read`/`band_read::multi` decode engine; also fixed a predictor cross-band-stride bug and an O(n)→O(1) tile-index lookup); the same interleaving/byte-order defect pattern independently found and fixed in a dozen downstream crates (QC, server, mobile, WASM, WCS, Node, CLI, ML, Jupyter, VRT); new `oxigeo-core` typed zero-copy `RasterElement` layer and zero-allocation `read_interleaved`/`read_band_into`/`read_window_into` readers; DEFLATE tile decode 1.45–1.79× faster (`oxiarc-*` 0.4.0); plus unrelated fixes (streaming `ChunkedReader` first-read failure, mbtiles spill-file leak, ML pruning concurrency corruption, wasm32 `oxigeo-compress` build); 75 crates, 18,133 tests |
| **v0.3.0** | Q3 2026 | Streaming v2, cloud-native tile server v2, extended STAC support |
| **v1.0.0** | Q4 2026 | LTS commitment, enterprise compliance certifications |

## Development

```bash
cargo build --all-features
cargo nextest run --all-features
cargo clippy --all-features -- -D warnings
tokei .
```

See `crates/oxigeo-examples/src/` for runnable examples.

## Documentation

| Resource | Location |
|---------|----------|
| API Reference | https://docs.rs/oxigeo |
| Getting Started | [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md) |
| Architecture | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| Drivers | [docs/DRIVERS.md](docs/DRIVERS.md) |
| Algorithms | [docs/ALGORITHMS.md](docs/ALGORITHMS.md) |
| GDAL Migration | [docs/MIGRATION_FROM_GDAL.md](docs/MIGRATION_FROM_GDAL.md) |
| CHANGELOG | [CHANGELOG.md](CHANGELOG.md) |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full guide. Short version — follow [COOLJAPAN policies](docs/BEST_PRACTICES.md):

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

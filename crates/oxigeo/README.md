# OxiGeo

**Pure Rust Geospatial Data Abstraction Library — Production-Grade GDAL Alternative**

[![Crates.io](https://img.shields.io/crates/v/oxigeo.svg)](https://crates.io/crates/oxigeo)
[![Documentation](https://docs.rs/oxigeo/badge.svg)](https://docs.rs/oxigeo)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/crates/l/oxigeo.svg)](LICENSE)

Umbrella crate for OxiGeo — open any supported geospatial format with a single
`Dataset::open()` call, just like `GDALOpen()`. Backed by **76 workspace crates**
and ~747,000 SLoC of production Rust, it covers 17 format drivers, full CRS
transformations, SIMD algorithms, cloud-native I/O, GPU acceleration, enterprise
security, and bindings for Python, Node.js, WASM, iOS, and Android. First released
as v0.1.0 on 2026-02-22; now at **v0.2.0**, in development under the OxiGeo name
(v0.1.7 production-hardening validation complete 2026-07-20, not yet published to
crates.io).

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
    println!("Format  : {}", dataset.format());
    println!("Size    : {}x{}", dataset.width(), dataset.height());
    println!("CRS     : {}", dataset.crs().name());
    println!("Drivers : {:?}", oxigeo::drivers());
    Ok(())
}
```

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `geotiff` | yes | GeoTIFF / Cloud Optimized GeoTIFF |
| `geojson` | yes | GeoJSON (RFC 7946) |
| `shapefile` | yes | ESRI Shapefile |
| `geoparquet` | no | GeoParquet (Apache Arrow) |
| `zarr` | no | Zarr v2/v3 arrays |
| `flatgeobuf` | no | FlatGeobuf (packed Hilbert R-tree) |
| `netcdf` | no | NetCDF (CF conventions) |
| `hdf5` | no | HDF5 hierarchical data |
| `grib` | no | GRIB1/GRIB2 meteorological |
| `jpeg2000` | no | JPEG2000 |
| `vrt` | no | Virtual Raster Tables |
| `full` | no | All 11 format drivers |
| `proj` | no | CRS transformations (20+ projections, 1000+ EPSG) |
| `algorithms` | no | SIMD raster/vector algorithms |
| `cloud` | no | S3, GCS, Azure Blob storage |
| `async` | no | Async I/O traits |
| `arrow` | no | Apache Arrow zero-copy interop |
| `gpu` | no | GPU acceleration (wgpu) |
| `ml` | no | Machine learning pipeline |
| `server` | no | OGC WMS 1.3.0 / WFS 2.0.0 tile server |
| `security` | no | AES-256-GCM, TLS 1.3, RBAC/ABAC |
| `distributed` | no | Distributed cluster support |
| `streaming` | no | Real-time stream processing |

## Ecosystem Overview

OxiGeo is a workspace of **76 crates** organized across:

| Layer | Crates |
|-------|--------|
| Core | `oxigeo-core`, `oxigeo-proj`, `oxigeo-algorithms`, `oxigeo-qc` |
| Format drivers | `oxigeo-geotiff`, `-geojson`, `-geoparquet`, `-zarr`, `-flatgeobuf`, `-shapefile`, `-netcdf`, `-hdf5`, `-grib`, `-jpeg2000`, `-vrt` |
| Cloud & storage | `oxigeo-cloud`, `-cloud-enhanced`, `-drivers-advanced`, `-compress`, `-cache-advanced`, `-rs3gw` |
| Domain modules | `oxigeo-3d`, `-terrain`, `-temporal`, `-analytics`, `-sensors`, `-metadata`, `-stac`, `-query` |
| Enterprise infra | `oxigeo-server`, `-gateway`, `-security`, `-observability`, `-workflow`, `-distributed`, `-cluster`, `-ha` |
| Streaming & IoT | `oxigeo-streaming`, `-kafka`, `-kinesis`, `-pubsub`, `-mqtt`, `-websocket`, `-etl`, `-sync` |
| Platform bindings | `oxigeo-wasm`, `-pwa`, `-offline`, `-node`, `-python`, `-jupyter`, `-mobile`, `-mobile-enhanced`, `-embedded`, `-edge` |
| GPU & ML | `oxigeo-gpu`, `-gpu-advanced`, `-ml`, `-ml-foundation` |
| DB connectors | `oxigeo-postgis`, `-db-connectors` |
| Tooling | `oxigeo-cli`, `-dev-tools`, `-bench`, `-examples` |

## COOLJAPAN Policies

- **Pure Rust**: 100% Rust in default features; C/Fortran behind feature flags
- **No `unwrap()`**: `clippy::unwrap_used = "deny"` workspace-wide
- **Workspace versions**: all via `*.workspace = true`
- **Latest crates**: all deps at latest crates.io versions
- **COOLJAPAN ecosystem**: `oxiblas` (not OpenBLAS), `oxicode` (not bincode), `oxiarc-*` (not zip), `OxiFFT` (not rustfft)

## License

Licensed under Apache-2.0.

Copyright (c) COOLJAPAN OU (Team Kitasan) — https://github.com/cool-japan

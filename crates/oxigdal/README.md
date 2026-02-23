# OxiGDAL

**Pure Rust Geospatial Data Abstraction Library — Production-Grade GDAL Alternative**

[![Crates.io](https://img.shields.io/crates/v/oxigdal.svg)](https://crates.io/crates/oxigdal)
[![Documentation](https://docs.rs/oxigdal/badge.svg)](https://docs.rs/oxigdal)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/crates/l/oxigdal.svg)](LICENSE)

Umbrella crate for OxiGDAL — open any supported geospatial format with a single
`Dataset::open()` call, just like `GDALOpen()`.  Backed by **68 workspace crates**
and ~500,000 SLoC of production Rust, it covers 11 format drivers, full CRS
transformations, SIMD algorithms, cloud-native I/O, GPU acceleration, enterprise
security, and bindings for Python, Node.js, WASM, iOS, and Android.  Released
**v0.1.0** on 2026-02-22.

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
    println!("Format  : {}", dataset.format());
    println!("Size    : {}x{}", dataset.width(), dataset.height());
    println!("CRS     : {}", dataset.crs().name());
    println!("Drivers : {:?}", oxigdal::drivers());
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

OxiGDAL is a workspace of **68 crates** organized across:

| Layer | Crates |
|-------|--------|
| Core | `oxigdal-core`, `oxigdal-proj`, `oxigdal-algorithms`, `oxigdal-qc` |
| Format drivers | `oxigdal-geotiff`, `-geojson`, `-geoparquet`, `-zarr`, `-flatgeobuf`, `-shapefile`, `-netcdf`, `-hdf5`, `-grib`, `-jpeg2000`, `-vrt` |
| Cloud & storage | `oxigdal-cloud`, `-cloud-enhanced`, `-drivers-advanced`, `-compress`, `-cache-advanced`, `-rs3gw` |
| Domain modules | `oxigdal-3d`, `-terrain`, `-temporal`, `-analytics`, `-sensors`, `-metadata`, `-stac`, `-query` |
| Enterprise infra | `oxigdal-server`, `-gateway`, `-security`, `-observability`, `-workflow`, `-distributed`, `-cluster`, `-ha` |
| Streaming & IoT | `oxigdal-streaming`, `-kafka`, `-kinesis`, `-pubsub`, `-mqtt`, `-websocket`, `-etl`, `-sync` |
| Platform bindings | `oxigdal-wasm`, `-pwa`, `-offline`, `-node`, `-python`, `-jupyter`, `-mobile`, `-mobile-enhanced`, `-embedded`, `-edge` |
| GPU & ML | `oxigdal-gpu`, `-gpu-advanced`, `-ml`, `-ml-foundation` |
| DB connectors | `oxigdal-postgis`, `-db-connectors` |
| Tooling | `oxigdal-cli`, `-dev-tools`, `-bench`, `-examples` |

## COOLJAPAN Policies

- **Pure Rust**: 100% Rust in default features; C/Fortran behind feature flags
- **No `unwrap()`**: `clippy::unwrap_used = "deny"` workspace-wide
- **Workspace versions**: all via `*.workspace = true`
- **Latest crates**: all deps at latest crates.io versions
- **COOLJAPAN ecosystem**: `oxiblas` (not OpenBLAS), `oxicode` (not bincode), `oxiarc-*` (not zip), `OxiFFT` (not rustfft)

## License

Licensed under Apache-2.0.

Copyright (c) COOLJAPAN OU (Team Kitasan) — https://github.com/cool-japan

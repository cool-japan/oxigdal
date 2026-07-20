# OxiGeo Drivers

This directory is the workspace home for the core geospatial format drivers that make up the OxiGeo I/O layer. Each driver lives in its own sub-crate so you can pull in only what your project needs.

**Version**: 0.2.0 — Last updated: 2026-07-20

---

## Driver crates

| Crate | Description | docs.rs |
|---|---|---|
| `oxigeo-geotiff` | GeoTIFF / Cloud-Optimized GeoTIFF (COG) — raster read/write with HTTP range support | [docs.rs/oxigeo-geotiff](https://docs.rs/oxigeo-geotiff) |
| `oxigeo-zarr` | Zarr v2 / v3 — chunked multidimensional array storage with pluggable backends | [docs.rs/oxigeo-zarr](https://docs.rs/oxigeo-zarr) |
| `oxigeo-hdf5` | HDF5 — Pure Rust minimal HDF5 reader/writer (C binding optional, feature-gated) | [docs.rs/oxigeo-hdf5](https://docs.rs/oxigeo-hdf5) |
| `oxigeo-jpeg2000` | JPEG2000 / JP2 / J2K — JP2 box parsing and JPEG2000 codestream decode/encode | [docs.rs/oxigeo-jpeg2000](https://docs.rs/oxigeo-jpeg2000) |
| `oxigeo-netcdf` | NetCDF — Pure Rust NetCDF-3 with optional NetCDF-4/HDF5 (feature-gated) | [docs.rs/oxigeo-netcdf](https://docs.rs/oxigeo-netcdf) |
| `oxigeo-geoparquet` | GeoParquet — columnar geo-vector storage on Apache Arrow / Parquet | [docs.rs/oxigeo-geoparquet](https://docs.rs/oxigeo-geoparquet) |
| `oxigeo-geojson` | GeoJSON (RFC 7946) — streaming vector read/write with validation | [docs.rs/oxigeo-geojson](https://docs.rs/oxigeo-geojson) |
| `oxigeo-shapefile` | Shapefile (ESRI) — `.shp`/`.dbf`/`.shx` read/write | [docs.rs/oxigeo-shapefile](https://docs.rs/oxigeo-shapefile) |
| `oxigeo-vrt` | VRT (Virtual Raster Table) — tile mosaics, band subsets, pixel functions | [docs.rs/oxigeo-vrt](https://docs.rs/oxigeo-vrt) |
| `oxigeo-grib` | GRIB1 / GRIB2 — meteorological gridded data read/write | [docs.rs/oxigeo-grib](https://docs.rs/oxigeo-grib) |
| `oxigeo-flatgeobuf` | FlatGeobuf (.fgb) — FlatBuffers-encoded vector with packed Hilbert R-tree | [docs.rs/oxigeo-flatgeobuf](https://docs.rs/oxigeo-flatgeobuf) |

There is no single `oxigeo-drivers` meta-crate to depend on. Add only the driver crates you need:

```toml
[dependencies]
oxigeo-geotiff   = "0.2.0"
oxigeo-geojson   = "0.2.0"
oxigeo-shapefile = "0.2.0"
```

---

## Quick start

### Read a GeoTIFF

```rust
use oxigeo_geotiff::{TiffFile, CogReader};
use oxigeo_core::io::FileDataSource;

let source = FileDataSource::open("dem.tif")?;
let tiff   = TiffFile::parse(&source)?;
let reader = CogReader::open(source)?;
let tile   = reader.read_tile(0, 0, 0)?;
println!("width={} height={}", tiff.width(), tiff.height());
```

### Stream a GeoJSON feature collection

```rust
use oxigeo_geojson::GeoJsonStreamReader;
use std::fs::File;

let file   = File::open("cities.geojson")?;
let reader = GeoJsonStreamReader::new(file)?;

for result in reader {
    let feature = result?;
    println!("{:?}", feature.geometry());
}
```

### Read a Shapefile

```rust
use oxigeo_shapefile::ShapefileReader;

let reader   = ShapefileReader::open("boundaries.shp")?;
let features = reader.features()?;
println!("{} features", features.len());
```

### Open a Zarr store

```rust
use oxigeo_zarr::{FilesystemStore, ZarrArray};

let store = FilesystemStore::open("dataset.zarr")?;
let array = ZarrArray::open(&store, "/data")?;
let chunk = array.read_chunk(&[0, 0])?;
```

### Parse a FlatGeobuf file

```rust
use oxigeo_flatgeobuf::FlatGeobufReader;
use std::fs::File;

let file   = File::open("roads.fgb")?;
let reader = FlatGeobufReader::new(file)?;

for feature in reader.features()? {
    let feature = feature?;
    println!("{:?}", feature.geometry());
}
```

---

## Feature flags

Every driver crate follows the same baseline convention:

| Flag | Default | Meaning |
|---|---|---|
| `std` | yes | Links `std`; disable for `no_std`/embedded targets |
| `async` | no | Enables async I/O via Tokio |

Format-specific flags per driver:

### `oxigeo-geotiff`

| Flag | Default | Notes |
|---|---|---|
| `deflate` | yes | DEFLATE/zlib tile decompression via `oxiarc-deflate` |
| `lzw` | yes | LZW decompression via `oxiarc-lzw` |
| `zstd` | no | ZSTD compression via `oxiarc-zstd` |
| `jpeg` | no | JPEG tile codec (`jpeg-decoder` / `jpeg-encoder`) |
| `webp` | no | WebP tile support |

### `oxigeo-zarr`

| Flag | Default | Notes |
|---|---|---|
| `v2` | yes | Zarr v2 spec support |
| `v3` | yes | Zarr v3 spec support |
| `filesystem` | yes | Local filesystem store |
| `memory` | yes | In-memory store |
| `gzip` | yes | GZip codec via `oxiarc-archive` |
| `zstd` | yes | ZSTD codec via `oxiarc-zstd` |
| `lz4` | no | LZ4 codec via `oxiarc-lz4` |
| `s3` | no | AWS S3 store (pulls `aws-sdk-s3`, implies `async`) |
| `http` | no | HTTP store (implies `async`) |
| `parallel` | no | Parallel chunk I/O via Rayon |
| `cache` | no | LRU chunk cache |
| `shuffle` | no | Shuffle filter |
| `delta` | no | Delta filter |
| `scale-offset` | no | Scale-offset filter |

### `oxigeo-hdf5`

| Flag | Default | Notes |
|---|---|---|
| `pure_rust` | no | Explicit marker for minimal pure Rust mode |
| `szip` | no | SZIP / AEC-Rice codec (pure Rust) |

Note: The `hdf5_sys` feature (C bindings to libhdf5) is intentionally absent from default features to preserve Pure Rust compliance.

### `oxigeo-jpeg2000`

`std` and `async` only — no additional flags in 0.2.0.

### `oxigeo-netcdf`

| Flag | Default | Notes |
|---|---|---|
| `netcdf3` | no | Links `netcdf3` crate for full NetCDF-3 |
| `cf_conventions` | no | CF-1.x attribute parsing |

Note: `netcdf4` (C binding to libnetcdf) is permanently feature-gated; not in default features.

### `oxigeo-geoparquet`

| Flag | Default | Notes |
|---|---|---|
| (Arrow + Parquet) | yes (via `std`) | Arrow IPC and Parquet read/write |
| `snappy` | no | Snappy page compression |
| `gzip` | no | GZIP page compression |
| `brotli` | no | Brotli page compression |
| `lz4` | no | LZ4 page compression |
| `zstd` | no | ZSTD page compression |

### `oxigeo-geojson`

| Flag | Default | Notes |
|---|---|---|
| `arrow` | no | Arrow IPC output via `oxigeo-core/arrow` |

### `oxigeo-shapefile`

| Flag | Default | Notes |
|---|---|---|
| `arrow` | no | Arrow IPC output via `oxigeo-core/arrow` |

### `oxigeo-vrt`

`std` and `async` only — VRT composes other drivers; configure compression at the driver level.

### `oxigeo-grib`

| Flag | Default | Notes |
|---|---|---|
| `grib1` | yes | GRIB Edition 1 support |
| `grib2` | yes | GRIB Edition 2 support |
| `complex_packing` | no | Complex packing for compressed grids |

### `oxigeo-flatgeobuf`

| Flag | Default | Notes |
|---|---|---|
| `http` | no | HTTP(S) streaming (implies `async`, pulls `reqwest`) |

---

## Design principles

All driver crates in this directory follow the same engineering constraints:

- **Pure Rust by default** — No C or Fortran in the default feature set. C/Fortran bindings are available only via explicit opt-in feature flags so the dependency graph stays auditable and cross-compilable.
- **No `unwrap()` in production code** — All fallible paths return typed `Result` values.
- **Workspace dependency management** — `*.workspace = true` throughout; no per-crate version pins.
- **File size limit** — Every source file is kept under 2000 lines; `splitrs` is used for refactoring.
- **COOLJAPAN ecosystem** — Compression and archiving go through `oxiarc-*` crates; no `flate2`, `zip`, `zstd`, or `bzip2` crates directly.

---

## Testing

Each driver has its own test suite. Run all driver tests from the workspace root:

```bash
cargo nextest run --all-features \
  -p oxigeo-geotiff \
  -p oxigeo-zarr \
  -p oxigeo-hdf5 \
  -p oxigeo-jpeg2000 \
  -p oxigeo-netcdf \
  -p oxigeo-geoparquet \
  -p oxigeo-geojson \
  -p oxigeo-shapefile \
  -p oxigeo-vrt \
  -p oxigeo-grib \
  -p oxigeo-flatgeobuf
```

Or run a single driver:

```bash
cargo nextest run --all-features -p oxigeo-geotiff
```

---

## Related crates

| Crate | Role |
|---|---|
| `oxigeo-core` | Core types, traits, data model |
| `oxigeo-drivers-advanced` | JPEG2000-in-GPKG, GeoPackage, KML/KMZ, GML |
| `oxigeo-proj` | CRS transformation (PROJ bindings) |
| `oxigeo-index` | Spatial indexing and querying |
| `oxigeo-cloud` | Cloud storage backends (S3, GCS, Azure) |
| `oxigeo` | Umbrella crate with unified `Dataset` API |

---

## License

Licensed under the Apache License, Version 2.0.
See [LICENSE-APACHE](../../LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>.

Copyright © 2026 COOLJAPAN OU (Team Kitasan)
Repository: <https://github.com/cool-japan/oxigeo>

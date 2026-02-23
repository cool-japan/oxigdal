# oxigdal-geotiff

Pure Rust GeoTIFF and Cloud Optimized GeoTIFF (COG) driver.

[![Crates.io](https://img.shields.io/crates/v/oxigdal-geotiff)](https://crates.io/crates/oxigdal-geotiff)
[![Documentation](https://docs.rs/oxigdal-geotiff/badge.svg)](https://docs.rs/oxigdal-geotiff)
[![License](https://img.shields.io/crates/l/oxigdal-geotiff)](LICENSE)

## Overview

`oxigdal-geotiff` provides comprehensive support for reading and writing GeoTIFF files, with special optimizations for Cloud Optimized GeoTIFFs (COGs).

### Features

- ✅ Classic TIFF and BigTIFF support
- ✅ Cloud Optimized GeoTIFF (COG) reading and writing
- ✅ Tiled and stripped layouts
- ✅ Multiple compression schemes (DEFLATE, LZW, ZSTD, JPEG)
- ✅ All standard data types (UInt8-UInt64, Float32/64, Complex)
- ✅ Overview/pyramid levels
- ✅ GeoKeys for coordinate reference systems
- ✅ HTTP range request optimization

## Installation

```toml
[dependencies]
oxigdal-geotiff = "0.1"

# With specific compression support:
oxigdal-geotiff = { version = "0.1", features = ["deflate", "lzw", "zstd"] }
```

## Features

- **`deflate`** (default): DEFLATE/zlib compression
- **`lzw`** (default): LZW compression
- **`zstd`**: ZSTD compression (better compression ratio)
- **`jpeg`**: JPEG compression (feature-gated; enables `jpeg-decoder` and `jpeg-encoder`)
- **`async`**: Async I/O support

## Quick Start

### Reading a GeoTIFF

```rust
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_core::io::FileDataSource;

let source = FileDataSource::open("elevation.tif")?;
let reader = GeoTiffReader::open(source)?;

println!("Size: {}x{}", reader.width(), reader.height());
println!("Bands: {}", reader.band_count());
println!("EPSG: {:?}", reader.epsg_code());

// Read a tile
let tile_data = reader.read_tile(0, 0, 0)?;
```

### Reading a Cloud Optimized GeoTIFF (COG)

```rust
use oxigdal_geotiff::CogReader;
use oxigdal_core::io::FileDataSource;

let source = FileDataSource::open("satellite.tif")?;
let reader = CogReader::open(source)?;

// Access metadata
println!("Size: {}x{}", reader.width(), reader.height());
println!("Overviews: {}", reader.overview_count());
println!("Tile size: {:?}", reader.tile_size());

// Read from specific overview level
let level = 1; // First overview (half resolution)
let tile = reader.read_tile(level, 0, 0)?;
```

### Writing a GeoTIFF

```rust
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{RasterDataType, GeoTransform, BoundingBox};
use std::fs::File;

let buffer = RasterBuffer::zeros(1024, 1024, RasterDataType::Float32);

let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)?;
let geo_transform = GeoTransform::from_bounds(&bbox, 1024, 1024)?;

let options = GeoTiffWriterOptions {
    geo_transform: Some(geo_transform),
    epsg_code: Some(4326),
    tile_width: Some(256),
    tile_height: Some(256),
    ..Default::default()
};

let file = File::create("output.tif")?;
let writer = GeoTiffWriter::new(file, options)?;
writer.write_buffer(&buffer)?;
```

### Writing a Cloud Optimized GeoTIFF (COG)

```rust
use oxigdal_geotiff::writer::{CogWriter, CogWriterOptions, OverviewResampling};
use oxigdal_geotiff::tiff::Compression;

let options = CogWriterOptions {
    geo_transform: Some(geo_transform),
    epsg_code: Some(4326),
    tile_width: 512,
    tile_height: 512,
    compression: Compression::Deflate,
    overview_resampling: OverviewResampling::Average,
    overview_levels: vec![2, 4, 8, 16],
    ..Default::default()
};

let file = File::create("output_cog.tif")?;
let writer = CogWriter::new(file, options)?;
writer.write_buffer(&buffer)?;
```

## Compression Options

```rust
use oxigdal_geotiff::tiff::Compression;

// Available compression methods:
Compression::None          // No compression
Compression::Deflate       // DEFLATE/zlib (good all-around)
Compression::Lzw          // LZW (good for categorical)
Compression::Zstd         // ZSTD (best compression)
Compression::Jpeg         // JPEG (lossy, for RGB imagery)
```

## COG Validation

```rust
use oxigdal_geotiff::{TiffFile, cog};
use oxigdal_core::io::FileDataSource;

let source = FileDataSource::open("maybe_cog.tif")?;
let tiff = TiffFile::parse(&source)?;
let validation = cog::validate_cog(&tiff, &source);

if validation.is_valid {
    println!("✓ Valid COG");
} else {
    for error in &validation.errors {
        println!("✗ {}", error);
    }
}
```

## GeoKeys

Read coordinate reference system information:

```rust
use oxigdal_geotiff::geokeys::GeoKeyDirectory;

let geo_keys = GeoKeyDirectory::from_ifd(
    tiff.primary_ifd(),
    &source,
    header.byte_order,
    header.variant
)?;

if let Some(epsg) = geo_keys.epsg_code() {
    println!("EPSG: {}", epsg);
}

if let Some(model_type) = geo_keys.model_type() {
    println!("Model type: {:?}", model_type);
}
```

## Performance

### HTTP Range Requests

COGs are optimized for cloud storage with HTTP range requests. Only needed tiles are fetched:

```rust
use oxigdal_core::io::HttpDataSource;

// Read from cloud storage
let source = HttpDataSource::new("https://example.com/satellite.tif").await?;
let reader = CogReader::open(source)?;

// Only fetches the specific tile bytes
let tile = reader.read_tile(0, 5, 3)?;
```

### Tile Caching

For repeated access, use a caching layer:

```rust
use oxigdal_core::io::{CachedDataSource, FileDataSource};

let source = FileDataSource::open("large.tif")?;
let cached = CachedDataSource::new(source, 100_000_000); // 100MB cache
let reader = CogReader::open(cached)?;
```

## Examples

```bash
cargo run --example read_geotiff path/to/file.tif
cargo run --example tile_processing
```

## COOLJAPAN Policies

- ✅ Pure Rust - No C dependencies
- ✅ No unwrap() - Comprehensive error handling
- ✅ Zero-copy where possible
- ✅ Production ready

## License

Licensed under Apache-2.0.

Copyright © 2025 COOLJAPAN OU (Team Kitasan)

## See Also

- [Driver Guide](/tmp/oxigdal_driver_guide.md)
- [API Documentation](https://docs.rs/oxigdal-geotiff)
- [TIFF Specification](https://www.awaresystems.be/imaging/tiff.html)
- [COG Specification](https://www.cogeo.org/)

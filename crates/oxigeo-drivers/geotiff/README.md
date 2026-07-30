# oxigeo-geotiff

Pure Rust GeoTIFF and Cloud Optimized GeoTIFF (COG) driver.

[![Crates.io](https://img.shields.io/crates/v/oxigeo-geotiff)](https://crates.io/crates/oxigeo-geotiff)
[![Documentation](https://docs.rs/oxigeo-geotiff/badge.svg)](https://docs.rs/oxigeo-geotiff)
[![License](https://img.shields.io/crates/l/oxigeo-geotiff)](LICENSE)

## Overview

`oxigeo-geotiff` provides comprehensive support for reading and writing GeoTIFF files, with special optimizations for Cloud Optimized GeoTIFFs (COGs).

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
oxigeo-geotiff = "0.2"

# With specific compression support:
oxigeo-geotiff = { version = "0.2", features = ["deflate", "lzw", "zstd"] }
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
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_core::io::FileDataSource;

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
use oxigeo_geotiff::CogReader;
use oxigeo_core::io::FileDataSource;

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
use oxigeo_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions};
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::{RasterDataType, GeoTransform, BoundingBox};
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
use oxigeo_geotiff::writer::{CogWriter, CogWriterOptions, OverviewResampling};
use oxigeo_geotiff::tiff::Compression;

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

### Reading one band, or a window of one band

`read_tile`/`read_band` return raw driver bytes exactly as one band; use
`read_band_into`/`read_window_into` to decode straight into a caller-owned
buffer with no extra allocation:

```rust
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_core::io::FileDataSource;

let source = FileDataSource::open("multiband.tif")?;
let reader = GeoTiffReader::open(source)?;

// Band 1 (0-indexed), whole image, no intermediate allocation
let mut band = vec![0u8; reader.band_byte_len(0)?];
reader.read_band_into(0, 1, &mut band)?;

// A 256x256 window of band 0
let window = reader.read_window(0, 0, 100, 100, 256, 256)?;
```

## Compression Options

```rust
use oxigeo_geotiff::tiff::Compression;

// Available compression methods:
Compression::None          // No compression
Compression::Deflate       // DEFLATE/zlib (good all-around)
Compression::Lzw          // LZW (good for categorical)
Compression::Zstd         // ZSTD (best compression)
Compression::Jpeg         // JPEG (lossy, for RGB imagery)
```

## COG Validation

```rust
use oxigeo_geotiff::{TiffFile, cog};
use oxigeo_core::io::FileDataSource;

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
use oxigeo_geotiff::geokeys::GeoKeyDirectory;

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
use oxigeo_core::io::HttpDataSource;

// Read from cloud storage
let source = HttpDataSource::new("https://example.com/satellite.tif").await?;
let reader = CogReader::open(source)?;

// Only fetches the specific tile bytes
let tile = reader.read_tile(0, 5, 3)?;
```

### Tile Caching

For repeated access, use a caching layer:

```rust
use oxigeo_core::io::{CachedDataSource, FileDataSource};

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

- [Driver Guide](/tmp/oxigeo_driver_guide.md)
- [API Documentation](https://docs.rs/oxigeo-geotiff)
- [TIFF Specification](https://www.awaresystems.be/imaging/tiff.html)
- [COG Specification](https://www.cogeo.org/)

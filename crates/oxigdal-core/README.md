# oxigdal-core

[![Crates.io](https://img.shields.io/crates/v/oxigdal-core.svg)](https://crates.io/crates/oxigdal-core)
[![Documentation](https://docs.rs/oxigdal-core/badge.svg)](https://docs.rs/oxigdal-core)
[![License](https://img.shields.io/crates/l/oxigdal-core.svg)](LICENSE)

Core abstractions for OxiGDAL - a Pure Rust GDAL reimplementation with zero-copy buffers and cloud-native support for modern geospatial computing.

## Features

- **Pure Rust** - No C/Fortran dependencies, builds anywhere Rust runs
- **Zero-copy buffers** - Efficient memory management with optional Apache Arrow integration
- **Cloud-native** - Designed for S3, GCS, Azure Blob, and other cloud object stores
- **No-std support** - Core types work in embedded and WASM environments
- **Type-safe** - Strongly typed pixel data types and georeferencing
- **No unwrap policy** - All fallible operations return `Result<T, E>`
- **SIMD-accelerated** - High-performance raster operations
- **Async-ready** - Optional async I/O traits for non-blocking operations

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-core = "0.1.3"
```

### Feature Flags

- `std` (default) - Enable standard library support
- `alloc` - Enable allocation support without full std (for no-std environments)
- `arrow` - Enable Apache Arrow integration for zero-copy interoperability
- `async` - Enable async I/O traits

Example with arrow support:

```toml
[dependencies]
oxigdal-core = { version = "0.1.3", features = ["arrow"] }
```

## Quick Start

```rust
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_core::buffer::RasterBuffer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a bounding box for global extent
    let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)?;

    // Create a geotransform for a 1-degree resolution grid
    let geo_transform = GeoTransform::from_bounds(&bbox, 360, 180)?;

    // Create a typed raster buffer
    let buffer = RasterBuffer::zeros(360, 180, RasterDataType::Float32);

    println!("Created {}x{} raster with bounds: {:?}",
             buffer.width(), buffer.height(), bbox);

    Ok(())
}
```

## Core Types

### Spatial Types

- **`BoundingBox`** - 2D spatial extent with intersection, union, and buffering operations
- **`BoundingBox3D`** - 3D spatial extent with elevation
- **`GeoTransform`** - Affine transformation for converting pixel coordinates to geographic coordinates
- **`PixelExtent`** - Raster extent in pixel space

### Raster Types

- **`RasterDataType`** - Pixel data types (UInt8, UInt16, Int16, Float32, Float64, etc.)
- **`RasterBuffer`** - Typed buffer for raster data with zero-copy operations
- **`RasterMetadata`** - Complete raster metadata (dimensions, CRS, bands, etc.)
- **`RasterStatistics`** - Min, max, mean, stddev for raster analysis
- **`NoDataValue`** - Type-safe representation of missing/invalid data
- **`ColorInterpretation`** - Band meaning (Gray, Red, Green, Blue, Alpha, etc.)
- **`PixelLayout`** - Memory organization (Interleaved, Band-sequential, etc.)

### Vector Types

- **`GeometryType`** - Point, LineString, Polygon, and Multi-geometry types

## Usage

### Working with Bounding Boxes

```rust
use oxigdal_core::types::BoundingBox;

// Create a bounding box
let bbox1 = BoundingBox::new(-10.0, -10.0, 10.0, 10.0)?;
let bbox2 = BoundingBox::new(0.0, 0.0, 20.0, 20.0)?;

// Intersection
let intersection = bbox1.intersection(&bbox2)?;
println!("Intersection: {:?}", intersection);

// Union
let union_bbox = bbox1.union(&bbox2);
println!("Union: {:?}", union_bbox);

// Buffer by 5 units
let buffered = bbox1.buffer(5.0)?;
println!("Buffered: {:?}", buffered);
```

### Geotransform Operations

```rust
use oxigdal_core::types::GeoTransform;

// Create north-up geotransform (simple case)
let gt = GeoTransform::north_up(
    -180.0,  // origin X
    90.0,    // origin Y
    0.1,     // pixel width
    -0.1,    // pixel height (negative for north-up)
);

// Convert pixel coordinates to geographic
let (geo_x, geo_y) = gt.pixel_to_geo(100, 50);
println!("Pixel (100, 50) -> Geo ({}, {})", geo_x, geo_y);

// Convert geographic to pixel coordinates
let (px, py) = gt.geo_to_pixel(10.0, 85.0)?;
println!("Geo (10.0, 85.0) -> Pixel ({}, {})", px, py);

// Get resolution
let (x_res, y_res) = gt.resolution();
println!("Resolution: {}x{}", x_res, y_res);
```

### Raster Buffers

```rust
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

// Create a buffer filled with zeros
let buffer = RasterBuffer::zeros(100, 100, RasterDataType::Float32);

// Create from existing data
let data: Vec<f32> = vec![1.0; 100 * 100];
let buffer = RasterBuffer::from_vec(100, 100, data)?;

// Access pixel value
if let Some(value) = buffer.get_pixel(50, 50) {
    println!("Value at (50, 50): {}", value);
}
```

### Raster Metadata

```rust
use oxigdal_core::types::{RasterMetadata, RasterDataType, GeoTransform};

let metadata = RasterMetadata {
    width: 1024,
    height: 768,
    band_count: 3,
    data_type: RasterDataType::UInt8,
    geo_transform: Some(GeoTransform::north_up(-180.0, 90.0, 0.35, -0.35)),
    crs_wkt: Some("EPSG:4326".to_string()),
    ..Default::default()
};

println!("Total pixels: {}", metadata.pixel_count());
println!("Bounds: {:?}", metadata.bounds());
println!("Resolution: {:?}", metadata.resolution());
```

## API Overview

| Module | Description |
|--------|-------------|
| `types` | Core geospatial types (BoundingBox, GeoTransform, RasterDataType) |
| `buffer` | Typed raster buffers with zero-copy operations |
| `simd_buffer` | SIMD-accelerated raster operations |
| `memory` | Memory management and allocation strategies |
| `io` | I/O traits for reading and writing geospatial data |
| `vector` | Vector geometry types and operations |
| `error` | Error types with descriptive messages |
| `tutorials` | Tutorial and example documentation |

## Performance

OxiGDAL-core is designed for high performance:

- **Zero-copy operations** - Minimize memory allocations
- **SIMD acceleration** - Leverage CPU vector instructions for bulk operations
- **Apache Arrow integration** - Seamless interop with data science tools
- **Cache-friendly layouts** - Optimized memory access patterns

Benchmarks (Apple M1 Pro):

| Operation | Time | Throughput |
|-----------|------|------------|
| Buffer creation (1M pixels) | 0.5ms | 2 GB/s |
| Pixel access | 2ns | 500M pixels/s |
| Geotransform | 3ns | 333M transforms/s |

Run benchmarks:
```bash
cargo bench --package oxigdal-core
```

## Project Statistics

- **~7,000 lines of code** (Rust)
- **~1,750 lines of documentation**
- **24 source files**
- **Zero unsafe code** in core types

## Error Handling

This library follows the "no unwrap" policy. All fallible operations return `Result<T, OxiGdalError>` with descriptive error messages:

```rust
use oxigdal_core::types::BoundingBox;

// Invalid bounding box (min > max)
let result = BoundingBox::new(10.0, -10.0, -10.0, 10.0);
assert!(result.is_err());
match result {
    Ok(_) => unreachable!(),
    Err(e) => eprintln!("Error: {}", e),
}
```

## Pure Rust

This library is **100% Pure Rust** with no C/Fortran dependencies. All functionality works out of the box without external libraries or system dependencies. Perfect for:

- Cross-compilation to any platform
- WebAssembly (WASM) targets
- Embedded systems
- Reproducible builds

## OxiGDAL Ecosystem

`oxigdal-core` is the foundation of the OxiGDAL ecosystem:

- **oxigdal-core** - Core types and traits (this crate)
- **oxigdal-algorithms** - Raster and vector algorithms
- **oxigdal-drivers** - Format drivers (GeoTIFF, NetCDF, Zarr, GeoJSON, etc.)
- **oxigdal-cloud** - Cloud object store integration
- **oxigdal-server** - Geospatial API server
- **oxigdal-wasm** - WebAssembly bindings
- **oxigdal-python** - Python bindings

See the [main OxiGDAL repository](https://github.com/cool-japan/oxigdal) for the complete ecosystem.

## COOLJAPAN Ecosystem

Part of the [COOLJAPAN](https://github.com/cool-japan) Pure Rust scientific computing ecosystem:

- **[SciRS2](https://github.com/cool-japan/scirs)** - Scientific computing primitives (NumPy-like)
- **[OxiBLAS](https://github.com/cool-japan/oxirs)** - Pure Rust BLAS operations
- **[Oxicode](https://github.com/cool-japan/oxirs)** - Binary serialization (bincode successor)
- **[OxiFFT](https://github.com/cool-japan/oxirs)** - Fast Fourier Transform

## Documentation

- **API Documentation**: [docs.rs/oxigdal-core](https://docs.rs/oxigdal-core)
- **Tutorials**: See the `tutorials` module for comprehensive guides
- **Main Project**: [github.com/cool-japan/oxigdal](https://github.com/cool-japan/oxigdal)

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](../../CONTRIBUTING.md) in the main repository.

### Development

```bash
# Run tests
cargo test --package oxigdal-core --all-features

# Run benchmarks
cargo bench --package oxigdal-core

# Check with all features
cargo clippy --package oxigdal-core --all-features -- -D warnings

# Test no-std compatibility
cargo check --package oxigdal-core --no-default-features --features alloc
```

## License

Licensed under the Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0).

## Authors

COOLJAPAN OU (Team Kitasan)

---

**Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem** - Building the future of scientific computing in Pure Rust.

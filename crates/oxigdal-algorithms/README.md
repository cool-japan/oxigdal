# oxigdal-algorithms

High-performance geospatial processing algorithms for raster and vector data.

[![Crates.io](https://img.shields.io/crates/v/oxigdal-algorithms)](https://crates.io/crates/oxigdal-algorithms)
[![Documentation](https://docs.rs/oxigdal-algorithms/badge.svg)](https://docs.rs/oxigdal-algorithms)
[![License](https://img.shields.io/crates/l/oxigdal-algorithms)](LICENSE)

## Overview

`oxigdal-algorithms` provides production-ready implementations of common geospatial algorithms:

- **Resampling**: Nearest, Bilinear, Bicubic, Lanczos
- **Raster operations**: Calculator, Hillshade, Slope/Aspect, Reclassification
- **Vector operations**: Buffer, Intersection, Union, Simplification
- **Performance**: SIMD optimizations, parallel processing

All algorithms are implemented in pure Rust with no external dependencies.

## Features

- **`simd`**: Enable SIMD optimizations (2-8x speedup)
- **`parallel`**: Enable parallel processing with rayon
- **`std`** (default): Standard library support

## Installation

```toml
[dependencies]
oxigdal-algorithms = "0.1"

# For maximum performance:
oxigdal-algorithms = { version = "0.1", features = ["simd", "parallel"] }
```

## Quick Start

### Resampling

```rust
use oxigdal_algorithms::resampling::{Resampler, ResamplingMethod};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

// Create source raster
let src = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);

// Resample to different dimensions
let resampler = Resampler::new(ResamplingMethod::Bilinear);
let dst = resampler.resample(&src, 500, 500)?;

println!("Resampled from {}x{} to {}x{}",
    src.width(), src.height(),
    dst.width(), dst.height());
```

### Hillshade

```rust
use oxigdal_algorithms::raster::Hillshade;

let dem = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);

let hillshade = Hillshade::new()
    .azimuth(315.0)   // Light from NW
    .altitude(45.0)   // 45° above horizon
    .z_factor(1.0)
    .cell_size(30.0); // 30m resolution

let shaded = hillshade.compute(&dem)?;
```

### Raster Calculator

```rust
use oxigdal_algorithms::raster::RasterCalculator;

let input1 = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
let input2 = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);

let calc = RasterCalculator::new();

// Simple operations
let sum = calc.add(&input1, &input2)?;
let scaled = calc.multiply_scalar(&input1, 2.5)?;

// Complex expressions
let result = calc.evaluate("(A + B) * 1.5 - 10", &[&input1, &input2])?;
```

## Resampling Methods

| Method | Speed | Quality | Best For |
|--------|-------|---------|----------|
| **Nearest** | ⭐⭐⭐⭐⭐ | ⭐⭐ | Categorical data |
| **Bilinear** | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | Continuous data, DEMs |
| **Bicubic** | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | High-quality imagery |
| **Lanczos** | ⭐⭐ | ⭐⭐⭐⭐⭐ | Maximum quality |

### Choosing a Method

```rust
// For classification/land cover
let resampler = Resampler::new(ResamplingMethod::Nearest);

// For elevation data
let resampler = Resampler::new(ResamplingMethod::Bilinear);

// For high-quality satellite imagery
let resampler = Resampler::new(ResamplingMethod::Bicubic);

// For maximum quality (publication)
let resampler = Resampler::new(ResamplingMethod::Lanczos);
```

## Raster Operations

### Slope and Aspect

```rust
use oxigdal_algorithms::raster::{Slope, Aspect};

let dem = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);

// Calculate slope in degrees
let slope_calc = Slope::new()
    .cell_size(30.0)
    .units_degrees(true);
let slope = slope_calc.compute(&dem)?;

// Calculate aspect (direction of steepest descent)
let aspect_calc = Aspect::new().cell_size(30.0);
let aspect = aspect_calc.compute(&dem)?;
```

### Reclassification

```rust
use oxigdal_algorithms::raster::Reclassify;
use std::collections::HashMap;

let mut rules = HashMap::new();
rules.insert((0.0, 10.0), 1.0);     // 0-10 → class 1
rules.insert((10.0, 20.0), 2.0);    // 10-20 → class 2
rules.insert((20.0, 100.0), 3.0);   // 20-100 → class 3

let reclass = Reclassify::new(rules);
let classified = reclass.compute(&input)?;
```

### Zonal Statistics

```rust
use oxigdal_algorithms::raster::ZonalStats;

let values = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
let zones = RasterBuffer::zeros(1000, 1000, RasterDataType::Int32);

let zonal = ZonalStats::new();
let stats = zonal.compute(&values, &zones)?;

for (zone_id, stat) in stats {
    println!("Zone {}: mean={:.2}, std={:.2}",
        zone_id, stat.mean, stat.std_dev);
}
```

## Vector Operations

### Buffer

```rust
use oxigdal_algorithms::vector::Buffer;
use oxigdal_core::vector::Geometry;

let point = Geometry::Point { x: 0.0, y: 0.0 };

let buffered = Buffer::new()
    .distance(100.0)
    .resolution(32)
    .compute(&point)?;
```

### Douglas-Peucker Simplification

```rust
use oxigdal_algorithms::vector::DouglasPeucker;

let simplified = DouglasPeucker::new()
    .tolerance(10.0)
    .compute(&linestring)?;

println!("Reduced from {} to {} points",
    linestring.point_count(), simplified.point_count());
```

## Performance

### SIMD Acceleration

Enable SIMD for 2-8x speedup:

```toml
[dependencies]
oxigdal-algorithms = { version = "0.1", features = ["simd"] }
```

**Supported:**
- x86_64: AVX2, SSE4.2
- ARM: NEON
- WebAssembly: SIMD128

### Parallel Processing

Enable rayon for near-linear scaling:

```toml
[dependencies]
oxigdal-algorithms = { version = "0.1", features = ["parallel"] }
```

```rust
let resampler = Resampler::new(ResamplingMethod::Bilinear)
    .parallel(true)
    .threads(8);
```

### Benchmarks

Run benchmarks (requires nightly):

```bash
cd benchmarks
cargo +nightly bench --bench resampling
```

**Example results** (4096x4096 to 1024x1024):

| Method | Time (no SIMD) | Time (SIMD) | Speedup |
|--------|---------------|-------------|---------|
| Nearest | 12ms | 3ms | 4.0x |
| Bilinear | 98ms | 24ms | 4.1x |
| Bicubic | 287ms | 76ms | 3.8x |

## Examples

See the `examples/` directory:

```bash
cargo run --example image_resampling --release
cargo run --example tile_processing
```

## COOLJAPAN Policies

- ✅ **Pure Rust** - No C/Fortran dependencies
- ✅ **No unwrap()** - All errors handled
- ✅ **SIMD optimized** - Maximum performance
- ✅ **Well tested** - Comprehensive test suite

## License

Licensed under Apache-2.0.

Copyright © 2025 COOLJAPAN OU (Team Kitasan)

## See Also

- [Algorithm Guide](/tmp/oxigdal_algorithm_guide.md)
- [API Documentation](https://docs.rs/oxigdal-algorithms)
- [GitHub Repository](https://github.com/cool-japan/oxigdal)

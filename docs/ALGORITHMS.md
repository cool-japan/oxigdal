# OxiGDAL Algorithm Guide

This guide covers all geospatial processing algorithms available in `oxigdal-algorithms`.

## Table of Contents

1. [Resampling Algorithms](#resampling-algorithms)
2. [Raster Operations](#raster-operations)
3. [Vector Operations](#vector-operations)
4. [Performance Optimization](#performance-optimization)

---

## Resampling Algorithms

Resampling changes the spatial resolution or dimensions of raster data while preserving spatial accuracy.

### Overview

The `oxigdal-algorithms` crate provides four resampling methods:

| Method | Speed | Quality | Best For |
|--------|-------|---------|----------|
| **Nearest** | ⭐⭐⭐⭐⭐ | ⭐⭐ | Categorical data, classification |
| **Bilinear** | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | Continuous data, DEMs |
| **Bicubic** | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | High-quality imagery |
| **Lanczos** | ⭐⭐ | ⭐⭐⭐⭐⭐ | Maximum quality imagery |

### Basic Usage

```rust
use oxigdal_algorithms::resampling::{Resampler, ResamplingMethod};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

fn resample_example() -> Result<(), Box<dyn std::error::Error>> {
    // Create source raster (1000x1000)
    let src = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);

    // Create resampler with bilinear interpolation
    let resampler = Resampler::new(ResamplingMethod::Bilinear);

    // Resample to 500x500
    let dst = resampler.resample(&src, 500, 500)?;

    println!("Resampled from {}x{} to {}x{}",
        src.width(), src.height(),
        dst.width(), dst.height());

    Ok(())
}
```

### Nearest Neighbor

**Characteristics:**
- Fastest method (no interpolation)
- Preserves exact pixel values
- Creates blocky appearance when upsampling
- No new values introduced

**Best for:**
- Land cover classification
- Categorical/thematic maps
- Integer data where interpolation is inappropriate

**Example:**

```rust
use oxigdal_algorithms::resampling::{NearestResampler, ResamplingMethod};

fn nearest_example() -> Result<(), Box<dyn std::error::Error>> {
    let src = RasterBuffer::zeros(1000, 1000, RasterDataType::UInt8);

    // Direct usage
    let nearest = NearestResampler;
    let dst = nearest.resample(&src, 500, 500)?;

    // Or via Resampler
    let resampler = Resampler::new(ResamplingMethod::Nearest);
    let dst2 = resampler.resample(&src, 500, 500)?;

    Ok(())
}
```

### Bilinear Interpolation

**Characteristics:**
- Smooth results
- Averages nearest 4 pixels (2x2 kernel)
- Good balance of speed and quality
- Slight blurring effect

**Best for:**
- Digital Elevation Models (DEMs)
- Continuous floating-point data
- Temperature/precipitation maps
- General-purpose resampling

**Example:**

```rust
use oxigdal_algorithms::resampling::BilinearResampler;

fn bilinear_example() -> Result<(), Box<dyn std::error::Error>> {
    // DEM resampling
    let dem = RasterBuffer::zeros(3000, 3000, RasterDataType::Float32);

    let bilinear = BilinearResampler;
    let resampled_dem = bilinear.resample(&dem, 1000, 1000)?;

    println!("Resampled DEM from 3000x3000 to 1000x1000");

    Ok(())
}
```

**Algorithm Details:**

Bilinear interpolation uses a weighted average of the nearest 4 pixels:

```
f(x,y) = (1-dx)(1-dy)·f₀₀ + dx(1-dy)·f₁₀ + (1-dx)dy·f₀₁ + dx·dy·f₁₁
```

Where:
- `dx`, `dy` are fractional positions within the pixel
- `f₀₀`, `f₁₀`, `f₀₁`, `f₁₁` are the surrounding pixel values

### Bicubic Interpolation

**Characteristics:**
- Very smooth results
- Uses 4x4 kernel (16 pixels)
- Better edge preservation than bilinear
- Can introduce slight ringing artifacts
- ~3x slower than bilinear

**Best for:**
- High-quality satellite imagery
- Aerial photos
- DEMs where smoothness is important
- Visual quality matters

**Example:**

```rust
use oxigdal_algorithms::resampling::BicubicResampler;

fn bicubic_example() -> Result<(), Box<dyn std::error::Error>> {
    // High-quality imagery resampling
    let image = RasterBuffer::zeros(4000, 4000, RasterDataType::UInt16);

    let bicubic = BicubicResampler::new();
    let resampled = bicubic.resample(&image, 2000, 2000)?;

    Ok(())
}
```

**Algorithm Details:**

Bicubic uses a cubic polynomial to interpolate values:

```
f(x,y) = Σᵢ Σⱼ aᵢⱼ xⁱ yʲ  (i,j = 0..3)
```

This produces smoother curves than bilinear while preserving edges better.

### Lanczos Resampling

**Characteristics:**
- Highest quality
- Uses 6x6 kernel (36 pixels)
- Excellent edge sharpness
- Can introduce ringing near sharp edges
- Slowest method (~10x slower than nearest)

**Best for:**
- Maximum quality requirements
- Final processing step
- Imagery for publication
- When file size/speed is not a concern

**Example:**

```rust
use oxigdal_algorithms::resampling::LanczosResampler;

fn lanczos_example() -> Result<(), Box<dyn std::error::Error>> {
    // Maximum quality resampling
    let image = RasterBuffer::zeros(8000, 8000, RasterDataType::Float32);

    // Lanczos with radius 3 (standard)
    let lanczos = LanczosResampler::new(3);
    let resampled = lanczos.resample(&image, 4000, 4000)?;

    Ok(())
}
```

**Algorithm Details:**

Lanczos uses a windowed sinc function:

```
L(x) = sinc(x) · sinc(x/a)  for |x| < a
     = 0                     otherwise
```

Where `a` is the kernel radius (typically 3).

### Choosing the Right Method

**Decision Tree:**

```
Is your data categorical/classified?
  ├─ Yes → Use Nearest
  └─ No → Is speed critical?
         ├─ Yes → Use Bilinear
         └─ No → Is maximum quality needed?
                ├─ Yes → Use Lanczos
                └─ No → Use Bicubic
```

**Data Type Recommendations:**

| Data Type | Recommended Method |
|-----------|-------------------|
| Land cover | Nearest |
| DEM (elevation) | Bilinear or Bicubic |
| Temperature | Bilinear |
| RGB imagery | Bicubic or Lanczos |
| Classification | Nearest |
| Floating-point continuous | Bilinear |

---

## Raster Operations

### Raster Calculator (Map Algebra)

Perform mathematical operations on raster data.

```rust
use oxigdal_algorithms::raster::RasterCalculator;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

fn raster_calc_example() -> Result<(), Box<dyn std::error::Error>> {
    let input1 = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
    let input2 = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);

    let calculator = RasterCalculator::new();

    // Add two rasters
    let result = calculator.add(&input1, &input2)?;

    // Multiply by scalar
    let scaled = calculator.multiply_scalar(&input1, 2.5)?;

    // Complex expression: (input1 + input2) * 1.5 - 10
    let complex = calculator.evaluate("(A + B) * 1.5 - 10", &[&input1, &input2])?;

    Ok(())
}
```

### Hillshade Generation

Create shaded relief visualization from DEMs.

```rust
use oxigdal_algorithms::raster::Hillshade;

fn hillshade_example() -> Result<(), Box<dyn std::error::Error>> {
    let dem = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);

    let hillshade = Hillshade::new()
        .azimuth(315.0)           // Light from NW
        .altitude(45.0)           // 45° above horizon
        .z_factor(1.0)            // Vertical exaggeration
        .cell_size(30.0);         // 30m resolution

    let shaded = hillshade.compute(&dem)?;

    Ok(())
}
```

**Parameters:**
- **Azimuth**: Direction of light source (0-360°, 0=North, 90=East)
- **Altitude**: Angle of light above horizon (0-90°)
- **Z-factor**: Vertical exaggeration multiplier
- **Cell size**: Spatial resolution in ground units

### Slope and Aspect

Calculate terrain slope and aspect from DEMs.

```rust
use oxigdal_algorithms::raster::{Slope, Aspect};

fn terrain_analysis() -> Result<(), Box<dyn std::error::Error>> {
    let dem = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);

    // Calculate slope (in degrees)
    let slope_calc = Slope::new()
        .cell_size(30.0)
        .units_degrees(true);
    let slope = slope_calc.compute(&dem)?;

    // Calculate aspect (direction of steepest descent)
    let aspect_calc = Aspect::new()
        .cell_size(30.0);
    let aspect = aspect_calc.compute(&dem)?;

    Ok(())
}
```

**Output:**
- **Slope**: 0-90° (or percent if units_degrees=false)
- **Aspect**: 0-360° (0=North, 90=East, 180=South, 270=West)

### Reclassification

Remap pixel values to new categories.

```rust
use oxigdal_algorithms::raster::Reclassify;
use std::collections::HashMap;

fn reclassify_example() -> Result<(), Box<dyn std::error::Error>> {
    let input = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);

    // Create reclassification rules
    let mut rules = HashMap::new();
    rules.insert((0.0, 10.0), 1.0);      // 0-10 → 1
    rules.insert((10.0, 20.0), 2.0);     // 10-20 → 2
    rules.insert((20.0, 100.0), 3.0);    // 20-100 → 3

    let reclass = Reclassify::new(rules);
    let output = reclass.compute(&input)?;

    Ok(())
}
```

### Zonal Statistics

Compute statistics within zones defined by a zone raster.

```rust
use oxigdal_algorithms::raster::ZonalStats;

fn zonal_stats_example() -> Result<(), Box<dyn std::error::Error>> {
    let values = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
    let zones = RasterBuffer::zeros(1000, 1000, RasterDataType::Int32);

    let zonal = ZonalStats::new();
    let stats = zonal.compute(&values, &zones)?;

    // Stats contains: min, max, mean, std_dev for each zone
    for (zone_id, stat) in stats {
        println!("Zone {}: mean={:.2}, std={:.2}",
            zone_id, stat.mean, stat.std_dev);
    }

    Ok(())
}
```

---

## Vector Operations

### Buffer Generation

Create buffers around vector geometries.

```rust
use oxigdal_algorithms::vector::Buffer;
use oxigdal_core::vector::Geometry;

fn buffer_example() -> Result<(), Box<dyn std::error::Error>> {
    let point = Geometry::Point { x: 0.0, y: 0.0 };

    // Fixed distance buffer
    let buffered = Buffer::new()
        .distance(100.0)
        .resolution(32)  // Number of segments for circles
        .compute(&point)?;

    Ok(())
}
```

### Intersection

Compute geometric intersection of two geometries.

```rust
use oxigdal_algorithms::vector::Intersection;

fn intersection_example() -> Result<(), Box<dyn std::error::Error>> {
    let poly1 = /* ... */;
    let poly2 = /* ... */;

    let intersection = Intersection::new();
    let result = intersection.compute(&poly1, &poly2)?;

    Ok(())
}
```

### Union

Combine multiple geometries into one.

```rust
use oxigdal_algorithms::vector::Union;

fn union_example() -> Result<(), Box<dyn std::error::Error>> {
    let geometries = vec![/* ... */];

    let union = Union::new();
    let result = union.compute(&geometries)?;

    Ok(())
}
```

### Douglas-Peucker Simplification

Simplify line geometries while preserving shape.

```rust
use oxigdal_algorithms::vector::DouglasPeucker;

fn simplify_example() -> Result<(), Box<dyn std::error::Error>> {
    let linestring = /* ... */;

    // Simplify with tolerance (max distance from original)
    let simplified = DouglasPeucker::new()
        .tolerance(10.0)
        .compute(&linestring)?;

    println!("Reduced from {} to {} points",
        linestring.point_count(), simplified.point_count());

    Ok(())
}
```

---

## Performance Optimization

### SIMD Acceleration

Enable SIMD for significant speedup (2-8x):

```toml
[dependencies]
oxigdal-algorithms = { version = "0.1", features = ["simd"] }
```

**Supported architectures:**
- x86_64: AVX2, SSE4.2
- ARM: NEON
- WebAssembly: SIMD128

**Speedup examples:**

| Operation | Without SIMD | With SIMD | Speedup |
|-----------|-------------|-----------|---------|
| Bilinear resample | 100ms | 25ms | 4x |
| Bicubic resample | 300ms | 80ms | 3.75x |
| Hillshade | 150ms | 50ms | 3x |
| Raster math | 80ms | 20ms | 4x |

### Parallel Processing

Enable parallel processing with rayon:

```toml
[dependencies]
oxigdal-algorithms = { version = "0.1", features = ["parallel"] }
```

```rust
use oxigdal_algorithms::resampling::Resampler;

fn parallel_example() -> Result<(), Box<dyn std::error::Error>> {
    let resampler = Resampler::new(ResamplingMethod::Bilinear)
        .parallel(true)
        .threads(8);

    let result = resampler.resample(&src, 1000, 1000)?;
    Ok(())
}
```

### Memory Optimization

**Chunked Processing:**

Process large rasters in chunks to control memory usage:

```rust
use oxigdal_algorithms::resampling::ChunkedResampler;

fn chunked_example() -> Result<(), Box<dyn std::error::Error>> {
    let resampler = ChunkedResampler::new(ResamplingMethod::Bilinear)
        .chunk_size(512, 512);

    // Process 10000x10000 raster in 512x512 chunks
    let result = resampler.resample_large(&src, 5000, 5000)?;

    Ok(())
}
```

**Zero-Copy with Arrow:**

Enable Arrow feature for zero-copy operations:

```toml
[dependencies]
oxigdal-core = { version = "0.1", features = ["arrow"] }
```

```rust
use oxigdal_core::buffer::RasterBuffer;
use arrow_array::Float32Array;

fn arrow_example() -> Result<(), Box<dyn std::error::Error>> {
    let buffer = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);

    // Zero-copy conversion to Arrow
    let arrow_array = buffer.to_float32_array()?;

    // Use with Arrow compute kernels
    // ...

    Ok(())
}
```

### Benchmarking

Run benchmarks to measure performance:

```bash
cd oxigdal/benchmarks
cargo bench --bench resampling
cargo bench --bench raster_ops
cargo bench --bench vector_ops
```

### Performance Tips

1. **Choose appropriate resampling method** - Don't use Lanczos unless quality matters
2. **Enable SIMD** - 2-4x speedup for free
3. **Use parallel processing** - Near-linear scaling with cores
4. **Process in chunks** - Control memory usage for large datasets
5. **Use NoData efficiently** - Check `is_nodata()` before processing
6. **Profile your code** - Use `cargo flamegraph` to find bottlenecks

---

## Algorithm Reference

### Resampling Methods

| Method | Kernel Size | Complexity | Quality |
|--------|------------|-----------|---------|
| Nearest | 1x1 | O(1) | Low |
| Bilinear | 2x2 | O(1) | Medium |
| Bicubic | 4x4 | O(1) | High |
| Lanczos | 6x6 | O(1) | Very High |

### Raster Operations

| Operation | Complexity | Memory | Parallelizable |
|-----------|-----------|--------|----------------|
| Raster Calc | O(n) | O(1) | ✅ |
| Hillshade | O(n) | O(1) | ✅ |
| Slope/Aspect | O(n) | O(1) | ✅ |
| Reclassify | O(n) | O(1) | ✅ |
| Zonal Stats | O(n) | O(z) | ⚠️ |

### Vector Operations

| Operation | Complexity | Memory | Robust |
|-----------|-----------|--------|---------|
| Buffer | O(n) | O(n) | ✅ |
| Intersection | O(n*m) | O(n+m) | ✅ |
| Union | O(n²) | O(n) | ✅ |
| Simplification | O(n log n) | O(n) | ✅ |

---

## See Also

- [Quickstart Guide](oxigdal_quickstart_guide.md)
- [Driver Guide](oxigdal_driver_guide.md)
- [WASM Guide](oxigdal_wasm_guide.md)
- API Documentation: https://docs.rs/oxigdal-algorithms

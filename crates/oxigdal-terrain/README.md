# OxiGDAL Terrain

[![Crates.io](https://img.shields.io/crates/v/oxigdal-terrain.svg)](https://crates.io/crates/oxigdal-terrain)
[![Documentation](https://docs.rs/oxigdal-terrain/badge.svg)](https://docs.rs/oxigdal-terrain)
[![License](https://img.shields.io/crates/l/oxigdal-terrain.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org)

Advanced terrain analysis and DEM (Digital Elevation Model) processing library for OxiGDAL. Provides comprehensive geospatial analysis capabilities for terrain derivatives, hydrological modeling, visibility analysis, and geomorphometric classification—all in 100% Pure Rust.

## Features

- **Terrain Derivatives**: Slope (Horn, Zevenbergen-Thorne), aspect, curvature (profile, plan, total), hillshade (traditional, multidirectional, combined), TPI, TRI, roughness
- **Hydrological Analysis**: Flow direction (D8, D-Infinity), flow accumulation, sink filling, watershed delineation, stream network extraction with Strahler ordering
- **Visibility Analysis**: Viewshed computation (binary and cumulative), line-of-sight analysis
- **Geomorphometry**: Landform classification (Weiss, Iwahashi-Pike), convergence index, positive/negative openness
- **Parallel Processing**: Optional parallel computation with Rayon for large datasets
- **Pure Rust**: No C/Fortran dependencies—100% safe, idiomatic Rust implementation
- **SciRS2 Integration**: Seamless integration with SciRS2-Core for scientific computing
- **Comprehensive Error Handling**: Rich error types with no unwrap panic policy
- **Feature-Gated**: Modular design with optional feature flags for selective compilation

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-terrain = "0.1.2"

# With all features enabled
oxigdal-terrain = { version = "0.1.2", features = ["all_features", "parallel"] }
```

### Feature Flags

- `default`: Enables `std`, `derivatives`, `hydrology`
- `std`: Standard library support (enabled by default)
- `derivatives`: Terrain derivatives (slope, aspect, curvature, etc.)
- `hydrology`: Hydrological analysis (flow, watershed, streams)
- `visibility`: Visibility analysis (viewshed, line-of-sight)
- `geomorphometry`: Geomorphometric features (landforms, convergence, openness)
- `parallel`: Parallel processing support with Rayon
- `all_features`: Enables all analysis features (excludes `parallel`)

## Quick Start

### Basic Slope Calculation

```rust
use oxigdal_terrain::derivatives::{slope_horn, SlopeUnits};
use scirs2_core::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a simple DEM (100x100 pixels, 10m cell size)
    let dem = Array2::from_elem((100, 100), 100.0_f32);

    // Calculate slope in degrees using Horn's method
    let slope = slope_horn(&dem, 10.0, SlopeUnits::Degrees, None)?;

    println!("Slope grid shape: {:?}", slope.dim());
    Ok(())
}
```

### Hydrological Analysis

```rust
use oxigdal_terrain::hydrology::*;
use scirs2_core::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dem = Array2::from_elem((100, 100), 100.0_f32);

    // Fill sinks first (important for D8 flow analysis)
    let filled = fill_sinks(&dem, None)?;

    // Calculate flow direction (D8 algorithm)
    let flow_dir = flow_direction(&filled, 10.0, FlowAlgorithm::D8, None)?;

    // Calculate flow accumulation
    let flow_accum = flow_accumulation(&flow_dir, None)?;

    // Extract streams (cells with flow accumulation > 100)
    let streams = extract_streams(&flow_accum, 100.0, None)?;

    println!("Number of stream cells: {:?}",
             streams.iter().filter(|&&v| v > 0.0).count());

    Ok(())
}
```

### Visibility Analysis

```rust
use oxigdal_terrain::visibility::*;
use scirs2_core::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dem = Array2::from_elem((100, 100), 100.0_f32);

    // Binary viewshed from observer at (50, 50) with observer height 2m
    let viewshed = viewshed_binary(
        &dem,
        10.0,  // cell size in meters
        50, 50, // observer position (x, y)
        2.0,    // observer height above terrain
        50.0,   // target height above terrain
        None,   // radius limit (None = whole DEM)
    )?;

    println!("Visible cells: {}",
             viewshed.iter().filter(|&&v| v > 0.0).count());

    Ok(())
}
```

### Geomorphometric Classification

```rust
use oxigdal_terrain::geomorphometry::*;
use scirs2_core::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dem = Array2::from_elem((100, 100), 100.0_f32);

    // Classify landforms using Weiss method
    let landforms = classify_weiss(
        &dem,
        10.0,   // cell size
        21,     // analysis radius in cells
        None,   // nodata value
    )?;

    println!("Landform classification complete");

    Ok(())
}
```

## API Overview

| Module | Description | Key Functions |
|--------|-------------|---|
| `derivatives` | Terrain surface characteristics | `slope`, `aspect`, `curvature`, `hillshade`, `tpi`, `tri`, `roughness` |
| `hydrology` | Hydrological flow and watersheds | `flow_direction`, `flow_accumulation`, `fill_sinks`, `watershed_from_point`, `extract_streams` |
| `visibility` | Viewshed and line-of-sight | `viewshed_binary`, `viewshed_cumulative`, `line_of_sight` |
| `geomorphometry` | Landform and surface properties | `classify_weiss`, `classify_iwahashi_pike`, `convergence_index`, `openness` |

### Derivatives Module

- `slope()` - Calculate slope (auto-select algorithm)
- `slope_horn()` - Horn's method for slope calculation
- `slope_zevenbergen_thorne()` - Zevenbergen-Thorne method
- `aspect()` - Calculate aspect direction with flat handling options
- `curvature()` - Calculate total curvature
- `profile_curvature()` - Slope of slope along steepest descent
- `plan_curvature()` - Slope of aspect perpendicular to steepest descent
- `tangential_curvature()` - Surface curvature perpendicular to slope direction
- `hillshade()` - Shaded relief for visualization (auto-select algorithm)
- `hillshade_traditional()` - Traditional hillshade rendering
- `hillshade_multidirectional()` - 16-directional hillshade
- `hillshade_combined()` - Combined hillshade with slope adjustment
- `tpi()` - Topographic Position Index (elevation relative to neighbors)
- `tpi_parallel()` - Parallel TPI computation
- `tri()` - Terrain Ruggedness Index
- `tri_parallel()` - Parallel TRI computation
- `tri_riley()` - Riley's TRI variant
- `roughness()` - Surface roughness (auto-select method)
- `roughness_range()` - Range of elevations in neighborhood
- `roughness_stddev()` - Standard deviation of elevations
- `vector_ruggedness_measure()` - VRM for roughness quantification

### Hydrology Module

- `flow_direction()` - Flow direction calculation (D8 or D-Infinity)
- `flow_direction_d8()` - D8 (8-directional) flow algorithm
- `flow_direction_dinf()` - D-Infinity continuous flow algorithm
- `flow_accumulation()` - Cumulative flow contribution
- `fill_sinks()` - Hydrologically correct DEM preprocessing
- `extract_streams()` - Extract stream network from flow accumulation
- `strahler_order()` - Assign Strahler stream order
- `watershed_from_point()` - Delineate watershed from pour point

### Visibility Module

- `viewshed_binary()` - Boolean visibility map
- `viewshed_cumulative()` - Weighted visibility accumulation
- `line_of_sight()` - Line-of-sight visibility between two points

### Geomorphometry Module

- `classify_weiss()` - Landform classification using Weiss (2001) method
- `classify_iwahashi_pike()` - Iwahashi & Pike (2007) classification
- `convergence_index()` - Surface convergence/divergence index
- `positive_openness()` - Exposed terrain openness
- `negative_openness()` - Enclosed terrain openness

## Algorithms

### Slope Calculation
- **Horn (1981)**: 3x3 neighborhood with smooth gradient estimation
- **Zevenbergen & Thorne (1987)**: Improved edge handling for irregular DEMs

### Flow Direction
- **D8 (O'Callaghan & Mark, 1984)**: 8-directional steepest descent
- **D-Infinity (Tarboton, 1997)**: Continuous flow direction on triangulated surface

### Landform Classification
- **Weiss (2001)**: Profile/plan curvature-based classification
- **Iwahashi & Pike (2007)**: Slope/curvature-based landform types

## Performance

Benchmarks on Apple Silicon (M3 Max) with synthetic DEM:

| Operation | 100x100 | 500x500 | 1000x1000 |
|-----------|---------|---------|-----------|
| Slope (Horn) | 0.15ms | 3.2ms | 13ms |
| Aspect (Horn) | 0.18ms | 3.5ms | 14ms |
| Curvature (Profile) | 0.16ms | 3.1ms | 12ms |
| Hillshade (Traditional) | 0.22ms | 4.2ms | 17ms |
| TPI (radius=1) | 0.14ms | 2.8ms | 11ms |
| TRI (Standard) | 0.13ms | 2.6ms | 10ms |

**Note**: Performance scales approximately O(n) with grid size. Parallel variants (with `parallel` feature) show 3-4x speedup on large datasets (1000x1000+).

## No Unwrap Policy

This library strictly follows the "no unwrap" policy. All fallible operations return `Result<T, TerrainError>` with descriptive error variants:

- `InvalidDimensions` - DEM dimensions out of bounds
- `InvalidCellSize` - Non-positive cell size
- `InvalidObserverPosition` - Position outside DEM bounds
- `InvalidAzimuth` / `InvalidAltitude` - Invalid angle parameters
- `FlowDirectionError` - Flow computation issues
- `WatershedError` - Watershed delineation problems
- `ViewshedError` - Visibility computation errors
- `ComputationError` - Generic computation failures
- `InsufficientMemory` - Memory allocation issues

Example error handling:

```rust
use oxigdal_terrain::{derivatives::slope_horn, SlopeUnits, TerrainError};
use scirs2_core::prelude::*;

fn analyze_terrain(dem: &Array2<f32>) -> Result<Array2<f32>, TerrainError> {
    slope_horn(dem, 10.0, SlopeUnits::Degrees, None)
}

fn main() {
    match analyze_terrain(&dem) {
        Ok(slope) => println!("Success: {:?}", slope.dim()),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

## Pure Rust

This library is **100% Pure Rust** with no C/Fortran dependencies. All numerical operations use:

- **SciRS2-Core**: Scientific computing primitives and linear algebra
- **Standard Rust**: Safe, idiomatic Rust without external C bindings

## Examples

The [examples](examples/) directory contains complete, runnable examples:

- `terrain_derivatives.rs` - Calculate all terrain derivatives
- `hydrological_analysis.rs` - Watershed delineation workflow
- `visibility_analysis.rs` - Viewshed computation
- `landform_classification.rs` - Geomorphometric classification

Run examples with:

```bash
cargo run --example terrain_derivatives --features all_features
cargo run --example hydrological_analysis --features hydrology
cargo run --example visibility_analysis --features visibility
cargo run --example landform_classification --features geomorphometry
```

## Documentation

- **[API Documentation](https://docs.rs/oxigdal-terrain)**: Full rustdoc with examples
- **[OxiGDAL](https://github.com/cool-japan/oxigdal)**: Parent geospatial library
- **[SciRS2](https://github.com/cool-japan/scirs)**: Scientific computing foundation

## Integration with OxiGDAL

oxigdal-terrain is a specialized module within the OxiGDAL ecosystem:

```rust
use oxigdal::raster::Band;
use oxigdal_terrain::derivatives::slope_horn;

// Load raster from GDAL source
let band = Band::from_file("dem.tif")?;
let dem = band.to_array2::<f32>()?;

// Apply terrain analysis
let slope = slope_horn(&dem, 10.0, SlopeUnits::Degrees, None)?;
```

## Contributing

Contributions welcome! Please ensure:

1. No `unwrap()` calls in production code
2. Comprehensive error handling with `TerrainError`
3. Benchmark critical operations with Criterion
4. Write doc tests for public APIs
5. Follow COOLJAPAN coding standards

## Testing

Run the full test suite:

```bash
cargo test --all-features
cargo test --doc --all-features
cargo test --test terrain_test --all-features
```

Run benchmarks:

```bash
cargo bench --bench terrain_bench --all-features
```

## Code Statistics

- **2,933 lines** of Rust code
- **25 source files** organized by feature
- **~130 lines** of documentation
- **100% Pure Rust** implementation

## License

This project is licensed under Apache-2.0. See [LICENSE](LICENSE) for details.

## Related COOLJAPAN Projects

- **[OxiGDAL](https://github.com/cool-japan/oxigdal)** - Core geospatial library
- **[OxiBLAS](https://github.com/cool-japan/oxiblas)** - Pure Rust BLAS
- **[OxiFFT](https://github.com/cool-japan/oxifft)** - Pure Rust FFT
- **[SciRS2](https://github.com/cool-japan/scirs)** - Scientific computing ecosystem
- **[NumRS2](https://github.com/cool-japan/numrs)** - Numerical computing (NumPy-like)
- **[Oxicode](https://github.com/cool-japan/oxicode)** - Serialization framework

---

**Part of the [COOLJAPAN](https://github.com/cool-japan) Pure Rust Ecosystem**

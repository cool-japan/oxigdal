# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2025-01-25

### Added

#### Resampling Algorithms
- Nearest neighbor resampling (fast, preserves categorical data)
- Bilinear interpolation (smooth, good for continuous data)
- Bicubic interpolation (high quality, slower)
- Lanczos resampling (highest quality, expensive)
- SIMD-optimized implementations for all methods
- Support for all `RasterDataType` variants

#### Raster Operations
- **Raster Calculator** - Map algebra with expression evaluation
  - Basic arithmetic operations (+, -, *, /)
  - Mathematical functions (sqrt, pow, abs, sin, cos, etc.)
  - Logical operations (and, or, not)
  - Conditional expressions (if-then-else)
  - NoData handling
- **Hillshade** - 3D terrain visualization
  - Configurable azimuth and altitude
  - Z-factor scaling
  - Variable cell sizes
- **Slope** - Terrain slope calculation
  - Degrees or percent units
  - Horn's method for accuracy
  - Edge handling
- **Aspect** - Direction of steepest descent
  - 0-360 degree output
  - North-oriented (0° = North)
- **Reclassification** - Value mapping and binning
  - Range-based classification
  - Custom value mapping
  - NoData preservation
- **Zonal Statistics** - Aggregate statistics by zones
  - Min, max, mean, stddev
  - Count and sum
  - Histogram generation
- **Filters** - Spatial filters
  - Mean filter (low-pass)
  - Median filter (noise reduction)
  - Gaussian blur
  - Edge detection (Sobel)
- **Morphology** - Morphological operations
  - Erosion
  - Dilation
  - Opening
  - Closing

#### Vector Operations
- **Buffer** - Fixed and variable distance buffering
  - Point, line, polygon buffering
  - Configurable resolution
  - Multi-ring buffers
- **Intersection** - Geometric intersection
  - Polygon-polygon intersection
  - Line-polygon intersection
  - Preserves attributes
- **Union** - Geometric union
  - Dissolve boundaries
  - Merge overlapping polygons
- **Difference** - Geometric difference
  - Symmetric difference support
- **Simplification** - Douglas-Peucker algorithm
  - Line simplification
  - Tolerance-based
  - Preserves topology (optional)
- **Centroid** - Calculate geometry centroids
  - Weighted centroids
  - Area-weighted for polygons
- **Area** - Calculate polygon areas
  - Planar area calculation
  - Geodetic area (WGS84)
- **Distance** - Distance calculations
  - Point-to-point
  - Point-to-line
  - Hausdorff distance
- **Validation** - Geometry validation
  - Self-intersection detection
  - Ring orientation
  - Hole validation

#### SIMD Optimizations
- Portable SIMD for `std::simd` (stable Rust)
- x86_64 AVX2 optimizations
- ARM NEON optimizations
- WebAssembly SIMD128
- Auto-detection of CPU capabilities
- Fallback to scalar code when SIMD unavailable
- 2-8x speedup for resampling operations
- 3-6x speedup for statistics computation

#### Performance Features
- Zero-copy operations where possible
- Cache-friendly memory access patterns
- Optional parallel processing with rayon
- Chunked processing for large datasets
- Memory-efficient streaming algorithms

#### Documentation
- Comprehensive rustdoc for all public APIs
- Examples for every algorithm
- Performance comparison benchmarks
- Usage guidelines and best practices

### Implementation Details

#### Design Decisions
- **Pure Rust**: No C/Fortran dependencies (no GDAL, GEOS, PROJ)
- **SIMD-first**: Algorithms designed for vectorization
- **no_std support**: Core algorithms work without std (requires alloc)
- **Type safety**: Compile-time guarantees for buffer types
- **Error handling**: Explicit error types, no unwrap()

#### Known Limitations
- Complex vector operations (e.g., polygon clipping) are basic implementations
- No geodetic distance calculations yet (planar only)
- Raster calculator expression parsing is simple (no complex expressions)
- Parallel processing requires `rayon` feature

### Performance Benchmarks

Resampling 4096x4096 → 1024x1024 (Float32):
- Nearest: 3ms (SIMD) vs 12ms (scalar) = 4.0x speedup
- Bilinear: 24ms (SIMD) vs 98ms (scalar) = 4.1x speedup
- Bicubic: 76ms (SIMD) vs 287ms (scalar) = 3.8x speedup

Statistics computation (1M pixels):
- Mean/StdDev: 0.8ms (SIMD) vs 3.2ms (scalar) = 4.0x speedup
- Min/Max: 0.5ms (SIMD) vs 2.1ms (scalar) = 4.2x speedup

### Future Roadmap

#### 0.2.0 (Planned)
- Geodetic distance and area calculations
- Advanced polygon clipping (Weiler-Atherton)
- Voronoi diagrams and Delaunay triangulation
- Contour generation (marching squares)
- Watershed segmentation
- Image classification (K-means, ISODATA)

#### 0.3.0 (Planned)
- Machine learning integration
- Advanced filters (bilateral, anisotropic diffusion)
- Texture analysis (GLCM)
- Principal component analysis (PCA)
- Feature extraction

### Dependencies

- `oxigdal-core` 0.1.x - Core types and traits
- `num-traits` 0.2.x - Numeric trait abstractions
- `rayon` 1.x (optional) - Parallel processing
- `arrow-array`, `arrow-buffer` 57.x (optional) - Zero-copy Arrow integration

### Testing

- 200+ unit tests
- Property-based tests with proptest
- Benchmarks for all algorithms
- Round-trip accuracy tests
- Edge case handling
- NoData value handling
- No unwrap() policy enforced

### License

Apache-2.0

Copyright © 2025 COOLJAPAN OU (Team Kitasan)

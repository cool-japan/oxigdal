//! Algorithm Tutorials and Guides
//!
//! This module provides comprehensive tutorials for using OxiGDAL algorithms
//! for raster and vector processing.
//!
//! # Table of Contents
//!
//! 1. [Resampling Operations](#resampling-operations)
//! 2. [Raster Calculations](#raster-calculations)
//! 3. [Terrain Analysis](#terrain-analysis)
//! 4. [Vector Operations](#vector-operations)
//! 5. [SIMD Optimization](#simd-optimization)
//! 6. [Parallel Processing](#parallel-processing)
//!
//! # Resampling Operations
//!
//! Resampling is the process of changing the spatial resolution or extent of a raster.
//! OxiGDAL provides several high-quality resampling methods.
//!
//! ## Available Methods
//!
//! - **Nearest Neighbor**: Fastest, best for categorical data
//! - **Bilinear**: Smooth, good for continuous data
//! - **Bicubic**: Higher quality, slower
//! - **Lanczos**: Highest quality, most expensive
//!
//! ## Example: Basic Resampling
//!
//! ```rust
//! use oxigdal_algorithms::resampling::{ResamplingMethod, Resampler};
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create source raster (1000x1000)
//! let src = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//!
//! // Downsample to 500x500 using bilinear interpolation
//! let resampler = Resampler::new(ResamplingMethod::Bilinear);
//! let dst = resampler.resample(&src, 500, 500)?;
//!
//! assert_eq!(dst.width(), 500);
//! assert_eq!(dst.height(), 500);
//! # Ok(())
//! # }
//! ```
//!
//! ## Example: Comparing Methods
//!
//! ```rust,ignore
//! use oxigdal_algorithms::resampling::{ResamplingMethod, Resampler};
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let src = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//!
//! // Try different methods
//! let methods = vec![
//!     ResamplingMethod::NearestNeighbor,
//!     ResamplingMethod::Bilinear,
//!     ResamplingMethod::Bicubic,
//!     ResamplingMethod::Lanczos,
//! ];
//!
//! for method in methods {
//!     let resampler = Resampler::new(method);
//!     let result = resampler.resample(&src, 500, 500)?;
//!     println!("Resampled with {:?}: {}x{}", method, result.width(), result.height());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Raster Calculations
//!
//! The raster calculator enables map algebra operations using expressions.
//!
//! ## Example: Simple Arithmetic
//!
//! ```rust,ignore
//! use oxigdal_algorithms::raster::calculator::RasterCalculator;
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut calc = RasterCalculator::new();
//!
//! // Add input rasters
//! let raster_a = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
//! let raster_b = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
//!
//! calc.add_raster("A", raster_a);
//! calc.add_raster("B", raster_b);
//!
//! // Compute: (A + B) / 2
//! let result = calc.evaluate("(A + B) / 2")?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Example: NDVI Calculation
//!
//! ```rust,ignore
//! use oxigdal_algorithms::raster::calculator::RasterCalculator;
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut calc = RasterCalculator::new();
//!
//! // Add NIR and Red bands
//! let nir = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
//! let red = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
//!
//! calc.add_raster("NIR", nir);
//! calc.add_raster("RED", red);
//!
//! // NDVI = (NIR - RED) / (NIR + RED)
//! let ndvi = calc.evaluate("(NIR - RED) / (NIR + RED)")?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Example: Complex Expression
//!
//! ```rust,ignore
//! use oxigdal_algorithms::raster::calculator::RasterCalculator;
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut calc = RasterCalculator::new();
//!
//! let dem = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//! calc.add_raster("DEM", dem);
//!
//! // Hillshade formula with azimuth and altitude
//! let expr = "sin(30 * 3.14159 / 180) - cos(30 * 3.14159 / 180) * cos(DEM)";
//! let result = calc.evaluate(expr)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Terrain Analysis
//!
//! OxiGDAL provides comprehensive terrain analysis tools for digital elevation models (DEMs).
//!
//! ## Hillshade Generation
//!
//! ```rust,ignore
//! use oxigdal_algorithms::raster::hillshade::Hillshade;
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let dem = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//!
//! // Generate hillshade with default parameters
//! // (azimuth=315°, altitude=45°, z-factor=1.0)
//! let hillshade = Hillshade::new()
//!     .azimuth(315.0)
//!     .altitude(45.0)
//!     .z_factor(1.0)
//!     .compute(&dem, 30.0)?;  // 30m cell size
//! # Ok(())
//! # }
//! ```
//!
//! ## Slope and Aspect
//!
//! ```rust,ignore
//! use oxigdal_algorithms::raster::slope_aspect::{SlopeAspect, SlopeUnit};
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let dem = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//!
//! // Compute slope in degrees
//! let sa = SlopeAspect::new();
//! let slope = sa.slope(&dem, 30.0, SlopeUnit::Degrees)?;
//!
//! // Compute aspect in degrees (0-360)
//! let aspect = sa.aspect(&dem, 30.0)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Zonal Statistics
//!
//! ```rust,ignore
//! use oxigdal_algorithms::raster::zonal_stats::{ZonalStats, ZonalStatistic};
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let values = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//! let zones = RasterBuffer::zeros(1000, 1000, RasterDataType::UInt8);
//!
//! let stats = ZonalStats::new()
//!     .add_statistic(ZonalStatistic::Mean)
//!     .add_statistic(ZonalStatistic::StdDev)
//!     .add_statistic(ZonalStatistic::Min)
//!     .add_statistic(ZonalStatistic::Max)
//!     .compute(&values, &zones)?;
//!
//! for (zone_id, zone_stats) in stats.iter() {
//!     println!("Zone {}: mean={}", zone_id, zone_stats.mean);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Vector Operations
//!
//! ## Buffer Generation
//!
//! ```rust,ignore
//! use oxigdal_algorithms::vector::buffer::VectorBuffer;
//! use oxigdal_core::vector::Geometry;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create point geometry
//! let point = Geometry::Point { x: 0.0, y: 0.0 };
//!
//! // Create 100-unit buffer around point
//! let buffer = VectorBuffer::new()
//!     .distance(100.0)
//!     .segments(32)  // Number of segments per quadrant
//!     .compute(&point)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Line Simplification
//!
//! ```rust,ignore
//! use oxigdal_algorithms::vector::douglas_peucker::DouglasPeucker;
//! use oxigdal_core::vector::Geometry;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create line with many points
//! let coords = vec![
//!     (0.0, 0.0),
//!     (1.0, 1.0),
//!     (2.0, 0.5),
//!     (3.0, 1.5),
//!     (4.0, 0.0),
//! ];
//!
//! let line = Geometry::LineString {
//!     coords: coords.clone(),
//! };
//!
//! // Simplify with tolerance of 0.5 units
//! let simplified = DouglasPeucker::new()
//!     .tolerance(0.5)
//!     .simplify(&line)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Geometric Predicates
//!
//! ```rust,ignore
//! use oxigdal_algorithms::vector::intersection::Intersection;
//! use oxigdal_core::vector::Geometry;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let poly1 = Geometry::Polygon {
//!     exterior: vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)],
//!     holes: vec![],
//! };
//!
//! let poly2 = Geometry::Polygon {
//!     exterior: vec![(5.0, 5.0), (15.0, 5.0), (15.0, 15.0), (5.0, 15.0), (5.0, 5.0)],
//!     holes: vec![],
//! };
//!
//! // Compute intersection
//! let result = Intersection::compute(&poly1, &poly2)?;
//! # Ok(())
//! # }
//! ```
//!
//! # SIMD Optimization
//!
//! Enable SIMD optimizations for maximum performance on modern CPUs.
//!
//! ## Enabling SIMD
//!
//! Add to your `Cargo.toml`:
//!
//! ```toml
//! oxigdal-algorithms = { version = "0.1", features = ["simd"] }
//! ```
//!
//! ## SIMD-Accelerated Operations
//!
//! Many operations automatically use SIMD when enabled:
//!
//! ```rust
//! use oxigdal_algorithms::resampling::{ResamplingMethod, Resampler};
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let src = RasterBuffer::zeros(4096, 4096, RasterDataType::Float32);
//!
//! // This will use SIMD instructions when the "simd" feature is enabled
//! let resampler = Resampler::new(ResamplingMethod::Bilinear);
//! let dst = resampler.resample(&src, 2048, 2048)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Available SIMD Operations
//!
//! - Element-wise arithmetic (add, subtract, multiply, divide)
//! - Resampling (bilinear, bicubic)
//! - Convolution filters
//! - Statistical reductions (min, max, sum, mean)
//! - Color space transformations
//!
//! # Parallel Processing
//!
//! Process large rasters efficiently using parallel tile processing.
//!
//! ## Parallel Tile Processing
//!
//! ```rust,ignore
//! use oxigdal_algorithms::parallel::tiles::TileProcessor;
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let input = RasterBuffer::zeros(8192, 8192, RasterDataType::Float32);
//!
//! // Process in 512x512 tiles using all CPU cores
//! let processor = TileProcessor::new()
//!     .tile_size(512, 512)
//!     .overlap(32);  // 32-pixel overlap to avoid edge artifacts
//!
//! let result = processor.process(&input, |tile| {
//!     // Process each tile independently
//!     // This closure runs in parallel
//!     tile.clone()
//! })?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Parallel Batch Processing
//!
//! ```rust,ignore
//! use oxigdal_algorithms::parallel::batch::BatchProcessor;
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Process multiple rasters in parallel
//! let rasters: Vec<RasterBuffer> = (0..10)
//!     .map(|_| RasterBuffer::zeros(1000, 1000, RasterDataType::Float32))
//!     .collect();
//!
//! let processor = BatchProcessor::new();
//! let results = processor.process_batch(&rasters, |raster| {
//!     // Process each raster independently
//!     raster.compute_statistics()
//! })?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Controlling Parallelism
//!
//! ```rust,ignore
//! use oxigdal_algorithms::parallel::raster::ParallelRaster;
//! use rayon::ThreadPoolBuilder;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Limit to 4 threads
//! let pool = ThreadPoolBuilder::new()
//!     .num_threads(4)
//!     .build()?;
//!
//! pool.install(|| {
//!     // All parallel operations inside this closure
//!     // will use at most 4 threads
//! });
//! # Ok(())
//! # }
//! ```
//!
//! # Best Practices
//!
//! ## Choose the Right Resampling Method
//!
//! - **Categorical data** (land cover, classification): Use NearestNeighbor
//! - **Continuous data** (elevation, temperature): Use Bilinear or Bicubic
//! - **High-quality imagery**: Use Lanczos
//! - **Performance-critical**: Use NearestNeighbor or Bilinear
//!
//! ## Optimize Tile Size
//!
//! - Too small: High overhead from thread management
//! - Too large: Poor parallelization and high memory usage
//! - Sweet spot: 256-1024 pixels per side, depending on data type
//!
//! ## Use SIMD Features
//!
//! Always enable SIMD for production workloads:
//!
//! ```toml
//! [dependencies]
//! oxigdal-algorithms = { version = "0.1", features = ["simd", "parallel"] }
//! ```
//!
//! ## Handle NoData Properly
//!
//! Always set and respect nodata values:
//!
//! ```rust
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::{RasterDataType, NoDataValue};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let nodata = NoDataValue::Float(-9999.0);
//! let buffer = RasterBuffer::nodata_filled(
//!     1000,
//!     1000,
//!     RasterDataType::Float32,
//!     nodata
//! );
//!
//! // Operations automatically handle nodata
//! let stats = buffer.compute_statistics()?;
//! println!("Valid pixels: {} / {}", stats.valid_count, buffer.pixel_count());
//! # Ok(())
//! # }
//! ```

//! Getting Started with `OxiGDAL` Core
//!
//! This module provides tutorials and examples for using `OxiGDAL` Core effectively.
//!
//! # Table of Contents
//!
//! 1. [Basic Concepts](#basic-concepts)
//! 2. [Working with Raster Data](#working-with-raster-data)
//! 3. [Coordinate Systems](#coordinate-systems)
//! 4. [Buffers and Memory Management](#buffers-and-memory-management)
//! 5. [Error Handling](#error-handling)
//! 6. [Performance Tips](#performance-tips)
//!
//! # Basic Concepts
//!
//! `OxiGDAL` Core provides fundamental abstractions for geospatial data processing.
//! The main concepts are:
//!
//! - **Bounding Box**: Defines spatial extent in geographic/projected coordinates
//! - **Geo Transform**: Maps between pixel and world coordinates
//! - **Raster Buffer**: Stores pixel data with type safety
//! - **Data Types**: Represents various pixel formats (`UInt8`, Float32, etc.)
//!
//! ## Example: Creating a Simple Raster
//!
//! ```rust
//! use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
//! use oxigdal_core::buffer::RasterBuffer;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Define spatial extent (global coverage)
//! let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)?;
//!
//! // Create geotransform for 1-degree resolution
//! let width = 360;
//! let height = 180;
//! let gt = GeoTransform::from_bounds(&bbox, width, height)?;
//!
//! // Create raster buffer
//! let buffer = RasterBuffer::zeros(width as u64, height as u64, RasterDataType::Float32);
//!
//! println!("Created {}x{} raster covering {:?}", width, height, bbox);
//! # Ok(())
//! # }
//! ```
//!
//! # Working with Raster Data
//!
//! ## Creating Buffers
//!
//! ```rust
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::{RasterDataType, NoDataValue};
//!
//! // Zero-filled buffer
//! let buffer = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//!
//! // Buffer with nodata value
//! let nodata = NoDataValue::Float(-9999.0);
//! let buffer_with_nodata = RasterBuffer::nodata_filled(
//!     1000,
//!     1000,
//!     RasterDataType::Float32,
//!     nodata
//! );
//! ```
//!
//! ## Reading and Writing Pixels
//!
//! ```rust
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! let mut buffer = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
//!
//! // Write pixel value at (50, 50)
//! buffer.set_pixel(50, 50, 255.0)?;
//!
//! // Read pixel value
//! let value = buffer.get_pixel(50, 50)?;
//! assert_eq!(value, 255.0);
//! # Ok::<(), oxigdal_core::error::OxiGdalError>(())
//! ```
//!
//! ## Computing Statistics
//!
//! ```rust
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! let mut buffer = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
//!
//! // Fill with test data
//! for y in 0..100 {
//!     for x in 0..100 {
//!         buffer.set_pixel(x, y, (x + y) as f64)?;
//!     }
//! }
//!
//! // Compute statistics
//! let stats = buffer.compute_statistics()?;
//! println!("Min: {}, Max: {}", stats.min, stats.max);
//! println!("Mean: {}, StdDev: {}", stats.mean, stats.std_dev);
//! println!("Valid pixels: {}", stats.valid_count);
//! # Ok::<(), oxigdal_core::error::OxiGdalError>(())
//! ```
//!
//! # Coordinate Systems
//!
//! ## Creating a `GeoTransform`
//!
//! `GeoTransform` defines the relationship between pixel coordinates and world coordinates.
//!
//! ```rust
//! use oxigdal_core::types::{BoundingBox, GeoTransform};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Method 1: From bounds and dimensions
//! let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)?;
//! let gt = GeoTransform::from_bounds(&bbox, 360, 180)?;
//!
//! // Method 2: North-up image (no rotation)
//! let gt = GeoTransform::north_up(-180.0, 90.0, 1.0, -1.0);
//!
//! // Method 3: Full specification (with rotation)
//! let gt = GeoTransform::new(
//!     -180.0,  // origin_x
//!     1.0,     // pixel_width
//!     0.0,     // row_rotation
//!     90.0,    // origin_y
//!     0.0,     // col_rotation
//!     -1.0,    // pixel_height (negative for north-up)
//! );
//! # Ok(())
//! # }
//! ```
//!
//! ## Converting Between Pixel and World Coordinates
//!
//! ```rust
//! use oxigdal_core::types::GeoTransform;
//!
//! # fn main() -> Result<(), oxigdal_core::error::OxiGdalError> {
//! let gt = GeoTransform::north_up(-180.0, 90.0, 1.0, -1.0);
//!
//! // Pixel to world
//! let (lon, lat) = gt.pixel_to_world(180.0, 90.0);
//! println!("Pixel (180, 90) -> World ({}, {})", lon, lat);
//!
//! // World to pixel
//! let (px, py) = gt.world_to_pixel(0.0, 0.0)?;
//! println!("World (0, 0) -> Pixel ({}, {})", px, py);
//! # Ok(())
//! # }
//! ```
//!
//! ## Working with Bounding Boxes
//!
//! ```rust
//! use oxigdal_core::types::BoundingBox;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let bbox1 = BoundingBox::new(-180.0, -90.0, 0.0, 90.0)?;
//! let bbox2 = BoundingBox::new(-90.0, -45.0, 90.0, 45.0)?;
//!
//! // Check intersection
//! if bbox1.intersects(&bbox2) {
//!     println!("Bounding boxes intersect!");
//!
//!     // Compute intersection
//!     if let Some(intersection) = bbox1.intersection(&bbox2) {
//!         println!("Intersection: {:?}", intersection);
//!     }
//! }
//!
//! // Compute union
//! let union = bbox1.union(&bbox2);
//! println!("Union: {:?}", union);
//!
//! // Expand to include a point
//! let mut bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0)?;
//! bbox.expand_to_include(15.0, 15.0);
//! # Ok(())
//! # }
//! ```
//!
//! # Buffers and Memory Management
//!
//! ## Type Conversion
//!
//! ```rust
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! // Create UInt8 buffer
//! let buffer_u8 = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);
//!
//! // Convert to Float32
//! let buffer_f32 = buffer_u8.convert_to(RasterDataType::Float32)?;
//!
//! assert_eq!(buffer_f32.data_type(), RasterDataType::Float32);
//! # Ok::<(), oxigdal_core::error::OxiGdalError>(())
//! ```
//!
//! ## Working with `NoData`
//!
//! ```rust
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::{RasterDataType, NoDataValue};
//!
//! let nodata = NoDataValue::Float(-9999.0);
//! let mut buffer = RasterBuffer::nodata_filled(
//!     100,
//!     100,
//!     RasterDataType::Float32,
//!     nodata
//! );
//!
//! // Check if a value is nodata
//! assert!(buffer.is_nodata(-9999.0));
//! assert!(!buffer.is_nodata(100.0));
//!
//! // Set valid pixel
//! buffer.set_pixel(50, 50, 100.0)?;
//!
//! // Compute statistics (ignores nodata)
//! let stats = buffer.compute_statistics()?;
//! println!("Valid pixels: {}", stats.valid_count);
//! # Ok::<(), oxigdal_core::error::OxiGdalError>(())
//! ```
//!
//! ## SIMD-Aligned Buffers
//!
//! For high-performance operations, use SIMD-aligned buffers:
//!
//! ```rust
//! use oxigdal_core::simd_buffer::AlignedBuffer;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create 64-byte aligned buffer (AVX-512)
//! let mut buffer = AlignedBuffer::<f32>::new(1024 * 1024, 64)?;
//!
//! // Fill with data
//! for (i, val) in buffer.as_mut_slice().iter_mut().enumerate() {
//!     *val = i as f32;
//! }
//!
//! // Access as slice
//! let slice = buffer.as_slice();
//! assert_eq!(slice.len(), 1024 * 1024);
//! # Ok(())
//! # }
//! ```
//!
//! # Error Handling
//!
//! All `OxiGDAL` functions return `Result<T, OxiGdalError>`. The error types are:
//!
//! ```rust
//! use oxigdal_core::error::{OxiGdalError, IoError, FormatError};
//!
//! fn example() -> oxigdal_core::Result<()> {
//!     // Use ? operator for error propagation
//!     let bbox = oxigdal_core::types::BoundingBox::new(0.0, 0.0, 10.0, 10.0)?;
//!
//!     // Pattern match on errors
//!     match do_something() {
//!         Ok(result) => println!("Success: {:?}", result),
//!         Err(OxiGdalError::Io(io_err)) => {
//!             eprintln!("I/O error: {}", io_err);
//!         }
//!         Err(OxiGdalError::InvalidParameter { parameter, message }) => {
//!             eprintln!("Invalid {}: {}", parameter, message);
//!         }
//!         Err(e) => {
//!             eprintln!("Other error: {}", e);
//!         }
//!     }
//!
//!     Ok(())
//! }
//!
//! fn do_something() -> oxigdal_core::Result<()> {
//!     Ok(())
//! }
//! ```
//!
//! # Performance Tips
//!
//! ## 1. Use Appropriate Data Types
//!
//! Choose the smallest data type that meets your needs:
//!
//! - `UInt8`: 0-255 range (e.g., RGB images)
//! - `Int16`: -32768 to 32767 (e.g., elevation data)
//! - `Float32`: General-purpose floating point (good balance of precision and size)
//! - `Float64`: High precision (use when necessary)
//!
//! ## 2. Leverage SIMD Buffers
//!
//! For performance-critical code, use SIMD-aligned buffers:
//!
//! ```rust
//! use oxigdal_core::simd_buffer::AlignedBuffer;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // 64-byte alignment for AVX-512
//! let buffer = AlignedBuffer::<f32>::new(1_000_000, 64)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## 3. Minimize Type Conversions
//!
//! Type conversions are expensive. Work in the native data type when possible.
//!
//! ## 4. Batch Operations
//!
//! Process multiple pixels at once rather than one at a time:
//!
//! ```rust
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! let mut buffer = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//!
//! // Slow: pixel-by-pixel
//! for y in 0..1000 {
//!     for x in 0..1000 {
//!         buffer.set_pixel(x, y, 100.0).ok();
//!     }
//! }
//!
//! // Fast: use fill_value
//! buffer.fill_value(100.0);
//! ```
//!
//! ## 5. Consider Memory Layout
//!
//! `OxiGDAL` uses row-major order (scanline-based). Access pixels in row order for better cache performance:
//!
//! ```rust
//! # use oxigdal_core::buffer::RasterBuffer;
//! # use oxigdal_core::types::RasterDataType;
//! # let buffer = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//! // Good: row-major order (cache-friendly)
//! for y in 0..buffer.height() {
//!     for x in 0..buffer.width() {
//!         // process pixel (x, y)
//!     }
//! }
//!
//! // Bad: column-major order (cache-unfriendly)
//! for x in 0..buffer.width() {
//!     for y in 0..buffer.height() {
//!         // process pixel (x, y)
//!     }
//! }
//! ```
//!
//! ## 6. Avoid Repeated Statistics Computation
//!
//! Statistics computation is O(n). Cache results when possible:
//!
//! ```rust
//! # use oxigdal_core::buffer::RasterBuffer;
//! # use oxigdal_core::types::RasterDataType;
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! # let buffer = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//! // Compute once
//! let stats = buffer.compute_statistics()?;
//!
//! // Use multiple times
//! println!("Min: {}, Max: {}", stats.min, stats.max);
//! println!("Mean: {}", stats.mean);
//! # Ok(())
//! # }
//! ```

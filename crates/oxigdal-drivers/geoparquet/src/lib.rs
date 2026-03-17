//! GeoParquet Driver for OxiGDAL
//!
//! This crate provides a pure Rust implementation of the GeoParquet 1.0 specification,
//! enabling efficient reading and writing of geospatial vector data in Apache Parquet format.
//!
//! # Features
//!
//! - Full GeoParquet 1.0 specification support
//! - WKB geometry encoding/decoding for all geometry types
//! - Spatial partitioning and indexing for efficient queries
//! - Zero-copy operations using Apache Arrow
//! - Compression support (Snappy, Gzip, Zstd, LZ4, Brotli)
//! - Spatial statistics and bounding box metadata
//! - Row group-level spatial filtering
//!
//! # Example
//!
//! ```rust,no_run
//! use oxigdal_geoparquet::{GeoParquetReader, GeoParquetWriter};
//! use oxigdal_geoparquet::metadata::{Crs, GeometryColumnMetadata};
//! use oxigdal_geoparquet::geometry::{Point, Geometry};
//! # use oxigdal_geoparquet::error::Result;
//!
//! # fn example() -> Result<()> {
//! // Create a writer with WGS84 CRS
//! let metadata = GeometryColumnMetadata::new_wkb()
//!     .with_crs(Crs::wgs84());
//!
//! let mut writer = GeoParquetWriter::new("output.parquet", "geometry", metadata)?;
//!
//! // Add geometries
//! let point = Geometry::Point(Point::new_2d(-122.4, 37.8));
//! writer.add_geometry(&point)?;
//!
//! // Finalize the file
//! writer.finish()?;
//!
//! // Read the file
//! let reader = GeoParquetReader::open("output.parquet")?;
//! let metadata = reader.metadata();
//! println!("CRS: {:?}", metadata.primary_column_metadata()?.crs);
//! # Ok(())
//! # }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
// Allow partial documentation during development
#![allow(missing_docs)]
// Allow dead code for future features
#![allow(dead_code)]
// Allow too many arguments for parquet operations
#![allow(clippy::too_many_arguments)]

// When no_std is active, bring in alloc for heap allocation (Vec, String, etc.)
#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(feature = "std")]
pub mod arrow_ext;
pub mod error;
pub mod geometry;
#[cfg(feature = "std")]
pub mod metadata;
#[cfg(feature = "std")]
pub mod partitioning;
#[cfg(feature = "std")]
pub mod spatial;

#[cfg(feature = "std")]
mod compression;
#[cfg(feature = "std")]
mod reader;
#[cfg(feature = "std")]
mod writer;

#[cfg(feature = "std")]
pub use compression::CompressionType;
pub use error::{GeoParquetError, Result};
#[cfg(feature = "std")]
pub use metadata::{Crs, GeoParquetMetadata, GeometryColumnMetadata};
#[cfg(feature = "std")]
pub use reader::GeoParquetReader;
#[cfg(feature = "std")]
pub use writer::GeoParquetWriter;

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// GeoParquet specification version
#[cfg(feature = "std")]
pub const GEOPARQUET_VERSION: &str = metadata::GEOPARQUET_VERSION;
/// GeoParquet specification version (no_std)
#[cfg(not(feature = "std"))]
pub const GEOPARQUET_VERSION: &str = "1.0.0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(GEOPARQUET_VERSION, "1.0.0");
    }
}

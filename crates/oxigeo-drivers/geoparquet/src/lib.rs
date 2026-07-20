//! GeoParquet Driver for OxiGeo
//!
//! This crate provides a pure Rust implementation of the GeoParquet 1.1
//! specification, enabling efficient reading and writing of geospatial vector
//! data in Apache Parquet format.
//!
//! Files written by older OxiGeo versions declaring GeoParquet `1.0.0` are
//! still accepted on read — see [`metadata::GeoParquetMetadata::validate`] for
//! the version compatibility policy.
//!
//! # Features
//!
//! - Full GeoParquet 1.1 specification support
//! - WKB geometry encoding/decoding for all geometry types (default writer path)
//! - GeoArrow native encodings: Point, LineString, Polygon, MultiPoint,
//!   MultiLineString, MultiPolygon (opt-in via
//!   [`writer::GeoParquetWriterBuilder::encoding`])
//! - GeoParquet 1.1 `covering.bbox` column detection + row-group pruning +
//!   `ArrowPredicate` fast-path (skip WKB decode entirely when bbox cols exist)
//! - Per-column / per-row-group statistics exposure via
//!   [`statistics::ColumnStatistics`]
//! - Spatial partitioning and indexing for efficient queries
//! - Zero-copy operations using Apache Arrow
//! - Compression support (Snappy, Gzip, Zstd, LZ4, Brotli)
//! - Spatial statistics and bounding box metadata
//! - Row group-level spatial filtering
//!
//! # Example
//!
//! ```rust,no_run
//! use oxigeo_geoparquet::{GeoParquetReader, GeoParquetWriter};
//! use oxigeo_geoparquet::metadata::{Crs, GeometryColumnMetadata};
//! use oxigeo_geoparquet::geometry::{Point, Geometry};
//! # use oxigeo_geoparquet::error::Result;
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
#[cfg(feature = "std")]
pub mod covering;
pub mod error;
#[cfg(feature = "std")]
pub mod filter;
pub mod geometry;
#[cfg(feature = "std")]
pub mod metadata;
#[cfg(feature = "std")]
pub mod partitioning;
#[cfg(feature = "std")]
pub mod plan;
#[cfg(feature = "std")]
pub mod predicate;
#[cfg(feature = "std")]
pub mod pushdown;
#[cfg(feature = "std")]
pub mod spatial;
#[cfg(feature = "std")]
pub mod statistics;

#[cfg(feature = "std")]
mod compression;
#[cfg(feature = "std")]
mod reader;
#[cfg(feature = "std")]
mod writer;

#[cfg(feature = "std")]
pub use compression::CompressionType;
#[cfg(feature = "std")]
pub use covering::BboxColumns;
pub use error::{GeoParquetError, Result};
#[cfg(feature = "std")]
pub use filter::{AttributePredicates, ColumnCondition, CompareOp, LogicOp};
#[cfg(feature = "std")]
pub use metadata::{
    CoordDim, Covering, CoveringBbox, Crs, EncodingType, GeoParquetMetadata, GeometryColumnMetadata,
};
#[cfg(feature = "std")]
pub use plan::{ColumnChunkRange, PushdownPlan, plan_pushdown, prune_row_groups};
#[cfg(feature = "std")]
pub use predicate::{AttributeFilter, CmpOp, ScalarValue};
#[cfg(feature = "std")]
pub use pushdown::execute_pushdown;
#[cfg(feature = "std")]
pub use reader::GeoParquetReader;
#[cfg(feature = "std")]
pub use statistics::ColumnStatistics;
#[cfg(feature = "std")]
pub use writer::{GeoParquetWriter, GeoParquetWriterBuilder};

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// GeoParquet specification version emitted by writers in this crate.
#[cfg(feature = "std")]
pub const GEOPARQUET_VERSION: &str = metadata::GEOPARQUET_VERSION;
/// GeoParquet specification version (no_std)
#[cfg(not(feature = "std"))]
pub const GEOPARQUET_VERSION: &str = "1.1.0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(GEOPARQUET_VERSION, "1.1.0");
    }
}

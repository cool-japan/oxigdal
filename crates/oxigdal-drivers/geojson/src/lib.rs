//! OxiGDAL GeoJSON Driver - RFC 7946 Implementation
//!
//! This crate provides a pure Rust implementation of GeoJSON (RFC 7946) reading
//! and writing for the OxiGDAL ecosystem. It supports all geometry types,
//! features, feature collections, and coordinate reference systems.
//!
//! # Features
//!
//! - Full RFC 7946 compliance
//! - Support for all geometry types (Point, LineString, Polygon, MultiPoint, MultiLineString, MultiPolygon, GeometryCollection)
//! - Feature and FeatureCollection support
//! - CRS (Coordinate Reference System) handling
//! - Streaming reader for large files
//! - Efficient writer with customizable formatting
//! - Comprehensive validation
//! - Zero-copy optimizations where possible
//! - No `unwrap()` or `panic!()` in production code
//!
//! # Example
//!
//! ```rust
//! use oxigdal_geojson::{GeoJsonReader, GeoJsonWriter, FeatureCollection};
//! use std::fs::File;
//! use std::io::{BufReader, BufWriter};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Reading GeoJSON
//! # let temp_dir = std::env::temp_dir();
//! # let input_path = temp_dir.join("test_input.geojson");
//! # std::fs::write(&input_path, r#"{"type":"FeatureCollection","features":[]}"#)?;
//! let file = File::open(&input_path)?;
//! let reader = BufReader::new(file);
//! let mut geojson_reader = GeoJsonReader::new(reader);
//! let feature_collection = geojson_reader.read_feature_collection()?;
//!
//! // Writing GeoJSON
//! # let output_path = temp_dir.join("test_output.geojson");
//! let file = File::create(&output_path)?;
//! let writer = BufWriter::new(file);
//! let mut geojson_writer = GeoJsonWriter::new(writer);
//! geojson_writer.write_feature_collection(&feature_collection)?;
//! # std::fs::remove_file(&input_path)?;
//! # std::fs::remove_file(&output_path)?;
//! # Ok(())
//! # }
//! ```
//!
//! # RFC 7946 Compliance
//!
//! This implementation follows RFC 7946 strictly:
//! - Right-hand rule for polygon orientation
//! - WGS84 as default CRS (EPSG:4326)
//! - Longitude-latitude order for coordinates
//! - Validation of geometry topologies
//! - Support for bounding boxes
//! - Foreign members preserved during round-trip
//!
//! # Performance
//!
//! - Streaming API for large files (O(1) memory for features)
//! - Zero-copy deserialization where possible
//! - Efficient buffering and I/O
//! - Parallel processing support (with `async` feature)
//!
//! # COOLJAPAN Policies
//!
//! - Pure Rust implementation (no C/C++ dependencies)
//! - No `unwrap()` or `expect()` in production code
//! - Comprehensive error handling
//! - Extensive testing (unit + integration + property-based)
//! - Clean API design following Rust idioms

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(clippy::all)]
// Pedantic disabled to reduce noise - default clippy::all is sufficient
// #![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
// Allow doc overindent in complex nested lists
#![allow(clippy::doc_overindented_list_items)]
// Allow partial documentation
#![allow(missing_docs)]
// Allow dead code for internal structures
#![allow(dead_code)]
// Allow manual div_ceil for CRS calculations
#![allow(clippy::manual_div_ceil)]
// Allow expect() for internal invariants
#![allow(clippy::expect_used)]
// Allow collapsible match for CRS handling
#![allow(clippy::collapsible_match)]
// Allow manual strip for path parsing
#![allow(clippy::manual_strip)]
// Allow should_implement_trait for builder patterns
#![allow(clippy::should_implement_trait)]

#[cfg(feature = "std")]
extern crate std;

pub mod error;
pub mod reader;
pub mod types;
pub mod utils;
pub mod validation;
pub mod writer;

// Re-export commonly used types
pub use error::{GeoJsonError, Result};
pub use reader::GeoJsonReader;
pub use types::{
    Coordinate, CoordinateSequence, Crs, Feature, FeatureCollection, Geometry, GeometryType,
    Position, Properties,
};
pub use validation::Validator;
pub use writer::GeoJsonWriter;

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate name
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// Default CRS for GeoJSON (WGS84)
pub const DEFAULT_CRS: &str = "urn:ogc:def:crs:OGC:1.3:CRS84";

/// GeoJSON MIME type
pub const MIME_TYPE: &str = "application/geo+json";

/// GeoJSON file extension
pub const FILE_EXTENSION: &str = ".geojson";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert!(!VERSION.is_empty());
        assert_eq!(NAME, "oxigdal-geojson");
        assert_eq!(DEFAULT_CRS, "urn:ogc:def:crs:OGC:1.3:CRS84");
        assert_eq!(MIME_TYPE, "application/geo+json");
        assert_eq!(FILE_EXTENSION, ".geojson");
    }
}

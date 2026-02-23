//! OxiGDAL Shapefile Driver - ESRI Shapefile Implementation
//!
//! This crate provides a pure Rust implementation of ESRI Shapefile reading and writing
//! for the OxiGDAL ecosystem. It supports the complete Shapefile format including:
//!
//! - `.shp` files (geometry)
//! - `.dbf` files (attributes)
//! - `.shx` files (spatial index)
//!
//! # Supported Geometry Types
//!
//! - Point, PointZ, PointM
//! - PolyLine, PolyLineZ, PolyLineM
//! - Polygon, PolygonZ, PolygonM
//! - MultiPoint, MultiPointZ, MultiPointM
//!
//! # Features
//!
//! - Pure Rust implementation (no C/Fortran dependencies)
//! - Support for all DBF field types (Character, Number, Logical, Date, Float)
//! - Proper encoding handling with code page support
//! - Comprehensive error handling
//! - No `unwrap()` or `panic!()` in production code
//! - Round-trip compatibility (read → modify → write)
//! - Spatial index (.shx) support
//!
//! # Example - Reading
//!
//! ```rust,no_run
//! use oxigdal_shapefile::ShapefileReader;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Open a Shapefile (reads .shp, .dbf, and .shx)
//! let reader = ShapefileReader::open("path/to/shapefile")?;
//!
//! // Read all features
//! let features = reader.read_features()?;
//!
//! for feature in &features {
//!     println!("Record {}: {:?}", feature.record_number, feature.geometry);
//!     println!("Attributes: {:?}", feature.attributes);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Example - Writing
//!
//! ```rust,no_run
//! use oxigdal_shapefile::{ShapefileWriter, ShapefileSchemaBuilder};
//! use oxigdal_shapefile::shp::shapes::ShapeType;
//! use std::env;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create schema
//! let schema = ShapefileSchemaBuilder::new()
//!     .add_character_field("NAME", 50)?
//!     .add_numeric_field("VALUE", 10, 2)?
//!     .build();
//!
//! // Create writer
//! let temp_dir = env::temp_dir();
//! let output_path = temp_dir.join("output");
//! let mut writer = ShapefileWriter::new(output_path, ShapeType::Point, schema)?;
//!
//! // Write features (example omitted for brevity)
//! // writer.write_features(&features)?;
//! # Ok(())
//! # }
//! ```
//!
//! # File Format
//!
//! A Shapefile consists of three required files:
//!
//! 1. **`.shp`** - Main file containing geometry data
//! 2. **`.dbf`** - dBase file containing attribute data
//! 3. **`.shx`** - Index file containing record offsets
//!
//! Additional optional files include `.prj` (projection), `.cpg` (code page), etc.
//!
//! # Binary Format Details
//!
//! ## .shp File Structure
//!
//! - Header (100 bytes)
//!   - File code: 9994 (big endian)
//!   - File length in 16-bit words (big endian)
//!   - Version: 1000 (little endian)
//!   - Shape type (little endian)
//!   - Bounding box (8 doubles, little endian)
//! - Records (variable length)
//!   - Record header (8 bytes, big endian)
//!   - Shape content (variable, little endian)
//!
//! ## .dbf File Structure
//!
//! - Header (32 bytes)
//!   - Version, date, record count, header size, record size
//! - Field descriptors (32 bytes each)
//! - Header terminator (0x0D)
//! - Records (fixed length based on field descriptors)
//! - File terminator (0x1A)
//!
//! ## .shx File Structure
//!
//! - Header (100 bytes, same as .shp)
//! - Index entries (8 bytes each)
//!   - Offset (4 bytes, big endian)
//!   - Content length (4 bytes, big endian)
//!
//! # COOLJAPAN Policies
//!
//! - Pure Rust implementation (no C/C++ dependencies)
//! - No `unwrap()` or `expect()` in production code
//! - Comprehensive error handling with descriptive errors
//! - Extensive testing (unit + integration + property-based)
//! - Clean API design following Rust idioms
//! - Files under 2000 lines (use splitrs for refactoring)
//!
//! # Performance Considerations
//!
//! - Buffered I/O for efficient reading/writing
//! - Spatial index (.shx) for fast random access
//! - Streaming API for large files (iterate records one at a time)
//! - Zero-copy optimizations where possible
//!
//! # Limitations
//!
//! - Currently only Point geometries are fully supported for conversion to OxiGDAL
//! - PolyLine, Polygon, and MultiPoint parsing is implemented but conversion pending
//! - MultiPatch (3D surfaces) support is limited
//! - No support for memo fields (.dbt files)
//!
//! # References
//!
//! - [ESRI Shapefile Technical Description](https://www.esri.com/content/dam/esrisites/sitecore-archive/Files/Pdfs/library/whitepapers/pdfs/shapefile.pdf)
//! - [dBase File Format](http://www.dbase.com/Knowledgebase/INT/db7_file_fmt.htm)

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(clippy::all)]
// Pedantic disabled to reduce noise - default clippy::all is sufficient
// #![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
// Allow slice patterns
#![allow(clippy::borrow_as_ptr)]
// Allow partial documentation
#![allow(missing_docs)]
// Allow collapsible match for explicit branching
#![allow(clippy::collapsible_match)]

#[cfg(feature = "std")]
extern crate std;

pub mod dbf;
pub mod error;
pub mod reader;
pub mod shp;
pub mod shx;
pub mod writer;

// Re-export commonly used types
pub use error::{Result, ShapefileError};
pub use reader::{ShapefileFeature, ShapefileReader};
pub use writer::{ShapefileSchemaBuilder, ShapefileWriter};

// Re-export shape types
pub use shp::shapes::{MultiPartShapeM, MultiPartShapeZ, Point, PointM, PointZ, ShapeType};
pub use shp::{Shape, ShapeRecord};

// Re-export DBF types
pub use dbf::{FieldDescriptor, FieldType, FieldValue};

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate name
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// Shapefile magic number (file code)
pub const FILE_CODE: i32 = 9994;

/// Shapefile version
pub const FILE_VERSION: i32 = 1000;

/// Shapefile file extension
pub const FILE_EXTENSION: &str = ".shp";

/// DBF file extension
pub const DBF_EXTENSION: &str = ".dbf";

/// SHX file extension
pub const SHX_EXTENSION: &str = ".shx";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert!(!VERSION.is_empty());
        assert_eq!(NAME, "oxigdal-shapefile");
        assert_eq!(FILE_CODE, 9994);
        assert_eq!(FILE_VERSION, 1000);
        assert_eq!(FILE_EXTENSION, ".shp");
        assert_eq!(DBF_EXTENSION, ".dbf");
        assert_eq!(SHX_EXTENSION, ".shx");
    }

    #[test]
    fn test_shape_type_exports() {
        // Ensure shape types are accessible
        let _point = ShapeType::Point;
        let _polygon = ShapeType::Polygon;
        let _pointz = ShapeType::PointZ;
    }

    #[test]
    fn test_field_type_exports() {
        // Ensure field types are accessible
        let _char = FieldType::Character;
        let _num = FieldType::Number;
        let _logical = FieldType::Logical;
    }
}

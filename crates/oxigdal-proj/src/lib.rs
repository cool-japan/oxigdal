//! Pure Rust coordinate transformation and projection support for OxiGDAL.
//!
//! This crate provides comprehensive coordinate reference system (CRS) and projection
//! capabilities for the OxiGDAL library. It includes support for:
//!
//! - EPSG code database with common coordinate reference systems
//! - WKT (Well-Known Text) parsing
//! - PROJ string support
//! - Coordinate transformations between different CRS
//! - Pure Rust implementation by default using proj4rs
//! - Optional C bindings to PROJ library (feature-gated)
//!
//! # Features
//!
//! - `std` (default): Enable standard library support
//! - `proj-sys`: Enable optional C bindings to PROJ library for full PROJ support
//!
//! # Examples
//!
//! ## Transform coordinates from WGS84 to Web Mercator
//!
//! ```
//! use oxigdal_proj::{Crs, Coordinate, Transformer};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create CRS from EPSG codes
//! let wgs84 = Crs::from_epsg(4326)?;
//! let web_mercator = Crs::from_epsg(3857)?;
//!
//! // Create transformer
//! let transformer = Transformer::new(wgs84, web_mercator)?;
//!
//! // Transform a coordinate (London: 0°, 51.5°N)
//! let london = Coordinate::from_lon_lat(0.0, 51.5);
//! let transformed = transformer.transform(&london)?;
//!
//! println!("Transformed: {}", transformed);
//! # Ok(())
//! # }
//! ```
//!
//! ## Use convenience functions
//!
//! ```
//! use oxigdal_proj::{Coordinate, transform_epsg};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let coord = Coordinate::from_lon_lat(-122.4194, 37.7749); // San Francisco
//! let transformed = transform_epsg(&coord, 4326, 3857)?;
//! println!("Transformed: {}", transformed);
//! # Ok(())
//! # }
//! ```
//!
//! ## Work with bounding boxes
//!
//! ```
//! use oxigdal_proj::{BoundingBox, Coordinate, Transformer, Crs};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let bbox = BoundingBox::new(-10.0, -10.0, 10.0, 10.0)?;
//!
//! let transformer = Transformer::from_epsg(4326, 3857)?;
//! let transformed_bbox = transformer.transform_bbox(&bbox)?;
//!
//! println!("Original: {:?}", bbox);
//! println!("Transformed: {:?}", transformed_bbox);
//! # Ok(())
//! # }
//! ```
//!
//! ## Use common CRS constants
//!
//! ```
//! use oxigdal_proj::Crs;
//!
//! let wgs84 = Crs::wgs84();
//! let web_mercator = Crs::web_mercator();
//! let nad83 = Crs::nad83();
//! let etrs89 = Crs::etrs89();
//! ```
//!
//! # EPSG Database
//!
//! The crate includes an embedded database of ~140 common EPSG codes, including:
//!
//! - WGS84 (EPSG:4326)
//! - Web Mercator (EPSG:3857)
//! - All WGS84 UTM zones (EPSG:32601-32660 North, 32701-32760 South)
//! - Common national datums (NAD83, ETRS89, GDA94, JGD2000, etc.)
//! - Common projected systems (British National Grid, US National Atlas, etc.)
//!
//! # Pure Rust Implementation
//!
//! By default, this crate uses the pure Rust `proj4rs` library for coordinate transformations.
//! This ensures:
//!
//! - No C/C++ dependencies
//! - Cross-platform compatibility
//! - Memory safety guarantees
//! - Easy integration with Rust projects
//!
//! For applications requiring full PROJ library compatibility, enable the `proj-sys` feature:
//!
//! ```toml
//! [dependencies]
//! oxigdal-proj = { version = "0.1", features = ["proj-sys"] }
//! ```
//!
//! # Accuracy and Limitations
//!
//! The pure Rust implementation using proj4rs provides accurate transformations for most
//! common use cases. However, it may have limitations compared to the full PROJ library:
//!
//! - Limited support for some exotic projections
//! - No dynamic datum grid shift support
//! - Simplified datum transformations
//!
//! For high-accuracy geodetic applications, consider using the `proj-sys` feature.

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod crs;
pub mod epsg;
pub mod error;
pub mod transform;
pub mod wkt;

// Re-export commonly used types
pub use crs::{Crs, CrsSource};
pub use epsg::{CrsType, EpsgDefinition, available_epsg_codes, contains_epsg, lookup_epsg};
pub use error::{Error, Result};
pub use transform::{
    BoundingBox, Coordinate, Coordinate3D, Transformer, transform_coordinate, transform_epsg,
};
pub use wkt::{WktNode, WktParser, parse_wkt};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Library name
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// Returns library information.
pub fn info() -> String {
    format!("{} v{}", NAME, VERSION)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_version_info() {
        let info = info();
        assert!(info.contains("oxigdal-proj"));
        assert!(info.contains("0.1.0"));
    }

    #[test]
    fn test_basic_workflow() {
        // Create CRS
        let wgs84 = Crs::wgs84();
        let web_mercator = Crs::web_mercator();

        // Create transformer
        let transformer = Transformer::new(wgs84, web_mercator);
        assert!(transformer.is_ok());

        // Transform coordinate
        let coord = Coordinate::from_lon_lat(0.0, 0.0);
        let result = transformer.expect("should create").transform(&coord);
        assert!(result.is_ok());
    }

    #[test]
    fn test_epsg_lookup() {
        let wgs84 = lookup_epsg(4326);
        assert!(wgs84.is_ok());

        assert!(contains_epsg(4326));
        assert!(contains_epsg(3857));
        assert!(!contains_epsg(99999));

        let codes = available_epsg_codes();
        assert!(!codes.is_empty());
    }

    #[test]
    fn test_convenience_functions() {
        let coord = Coordinate::from_lon_lat(0.0, 0.0);
        let result = transform_epsg(&coord, 4326, 4326);
        assert!(result.is_ok());
    }

    #[test]
    fn test_bounding_box_workflow() {
        let bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        assert!(bbox.is_ok());

        let bbox = bbox.expect("valid bbox");
        let transformer = Transformer::from_epsg(4326, 4326);
        assert!(transformer.is_ok());

        let result = transformer.expect("should create").transform_bbox(&bbox);
        assert!(result.is_ok());
    }

    #[test]
    fn test_wkt_parsing() {
        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]]]"#;
        let result = parse_wkt(wkt);
        assert!(result.is_ok());

        let node = result.expect("should parse");
        assert_eq!(node.node_type, "GEOGCS");
    }

    #[test]
    fn test_crs_creation_methods() {
        // From EPSG
        let crs1 = Crs::from_epsg(4326);
        assert!(crs1.is_ok());

        // From PROJ string
        let crs2 = Crs::from_proj("+proj=longlat +datum=WGS84 +no_defs");
        assert!(crs2.is_ok());

        // From WKT
        let wkt = r#"GEOGCS["WGS 84"]"#;
        let crs3 = Crs::from_wkt(wkt);
        assert!(crs3.is_ok());

        // Custom CRS
        let crs4 = Crs::custom("My CRS", "+proj=longlat +datum=WGS84 +no_defs");
        assert!(matches!(crs4.source(), CrsSource::Custom { .. }));
    }

    #[test]
    fn test_coordinate_types() {
        // 2D coordinate
        let coord_2d = Coordinate::new(10.0, 20.0);
        assert_eq!(coord_2d.x, 10.0);
        assert_eq!(coord_2d.y, 20.0);

        // 3D coordinate
        let coord_3d = Coordinate3D::new(10.0, 20.0, 30.0);
        assert_eq!(coord_3d.x, 10.0);
        assert_eq!(coord_3d.y, 20.0);
        assert_eq!(coord_3d.z, 30.0);

        // Conversion
        let coord_2d_from_3d = coord_3d.to_2d();
        assert_eq!(coord_2d_from_3d.x, 10.0);
        assert_eq!(coord_2d_from_3d.y, 20.0);
    }
}

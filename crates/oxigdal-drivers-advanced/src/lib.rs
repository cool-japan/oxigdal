//! OxiGDAL Advanced Format Drivers
//!
//! This crate provides advanced geospatial format drivers for OxiGDAL:
//! - JPEG2000 (JP2) - Pure Rust JPEG2000 codec with GeoJP2 support
//! - GeoPackage (GPKG) - SQLite-based vector and raster storage
//! - KML/KMZ - Keyhole Markup Language for Google Earth
//! - GML - Geography Markup Language (OGC standard)
//!
//! # Features
//!
//! - `jpeg2000` - JPEG2000 format support (enabled by default)
//! - `geopackage` - GeoPackage format support (enabled by default)
//! - `kml` - KML/KMZ format support (enabled by default)
//! - `gml` - GML format support (enabled by default)
//! - `async` - Async I/O support (optional)
//!
//! # Examples
//!
//! ## Reading JPEG2000
//!
//! ```no_run
//! use oxigdal_drivers_advanced::jp2;
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let file = File::open("image.jp2")?;
//! let image = jp2::read_jp2(file)?;
//! println!("Dimensions: {}x{}", image.width, image.height);
//! # Ok(())
//! # }
//! ```
//!
//! ## Reading GeoPackage
//!
//! ```no_run
//! use oxigdal_drivers_advanced::gpkg::GeoPackage;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let gpkg = GeoPackage::open("data.gpkg")?;
//! let tables = gpkg.feature_tables()?;
//! println!("Feature tables: {:?}", tables);
//! # Ok(())
//! # }
//! ```
//!
//! ## Reading KML
//!
//! ```no_run
//! use oxigdal_drivers_advanced::kml;
//! use std::io::BufReader;
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let file = BufReader::new(File::open("placemarks.kml")?);
//! let doc = kml::read_kml(file)?;
//! println!("Placemarks: {}", doc.placemark_count());
//! # Ok(())
//! # }
//! ```
//!
//! ## Reading GML
//!
//! ```no_run
//! use oxigdal_drivers_advanced::gml;
//! use std::io::BufReader;
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let file = BufReader::new(File::open("features.gml")?);
//! let collection = gml::read_gml(file)?;
//! println!("Features: {}", collection.len());
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::panic))]

pub mod error;

#[cfg(feature = "jpeg2000")]
pub mod jp2;

#[cfg(feature = "geopackage")]
pub mod gpkg;

#[cfg(feature = "kml")]
pub mod kml;

#[cfg(feature = "kml")]
pub mod kmz;

#[cfg(feature = "gml")]
pub mod gml;

pub use error::{Error, Result};

/// Library version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Check if a format is supported based on file extension.
pub fn is_supported(extension: &str) -> bool {
    match extension.to_lowercase().as_str() {
        #[cfg(feature = "jpeg2000")]
        "jp2" | "j2k" | "jpf" | "jpx" => true,
        #[cfg(feature = "geopackage")]
        "gpkg" => true,
        #[cfg(feature = "kml")]
        "kml" | "kmz" => true,
        #[cfg(feature = "gml")]
        "gml" | "xml" => true,
        _ => false,
    }
}

/// Get list of supported format extensions.
pub fn supported_extensions() -> Vec<&'static str> {
    let mut extensions = Vec::new();

    #[cfg(feature = "jpeg2000")]
    {
        extensions.extend_from_slice(&["jp2", "j2k", "jpf", "jpx"]);
    }

    #[cfg(feature = "geopackage")]
    {
        extensions.push("gpkg");
    }

    #[cfg(feature = "kml")]
    {
        extensions.extend_from_slice(&["kml", "kmz"]);
    }

    #[cfg(feature = "gml")]
    {
        extensions.extend_from_slice(&["gml", "xml"]);
    }

    extensions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_supported_extensions() {
        let extensions = supported_extensions();
        assert!(!extensions.is_empty());

        #[cfg(feature = "jpeg2000")]
        assert!(extensions.contains(&"jp2"));

        #[cfg(feature = "geopackage")]
        assert!(extensions.contains(&"gpkg"));

        #[cfg(feature = "kml")]
        assert!(extensions.contains(&"kml"));

        #[cfg(feature = "gml")]
        assert!(extensions.contains(&"gml"));
    }

    #[test]
    fn test_is_supported() {
        #[cfg(feature = "jpeg2000")]
        {
            assert!(is_supported("jp2"));
            assert!(is_supported("JP2"));
            assert!(is_supported("j2k"));
        }

        #[cfg(feature = "geopackage")]
        assert!(is_supported("gpkg"));

        #[cfg(feature = "kml")]
        {
            assert!(is_supported("kml"));
            assert!(is_supported("kmz"));
        }

        #[cfg(feature = "gml")]
        assert!(is_supported("gml"));

        assert!(!is_supported("txt"));
        assert!(!is_supported("unknown"));
    }
}

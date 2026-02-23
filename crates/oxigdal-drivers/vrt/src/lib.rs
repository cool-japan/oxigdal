//! OxiGDAL VRT Driver - Pure Rust VRT (Virtual Raster) Support
//!
//! This crate provides a pure Rust implementation of GDAL's VRT (Virtual Raster)
//! format, enabling efficient multi-file processing and on-the-fly transformations.
//!
//! # VRT Format
//!
//! VRT (Virtual Raster) is an XML-based format that references other raster files
//! without copying data. This enables:
//!
//! - **Mosaicking**: Combine multiple tiles into a single virtual dataset
//! - **Subsetting**: Extract specific bands from multi-band rasters
//! - **Transformation**: Apply on-the-fly scaling, offset, and pixel functions
//! - **Windowing**: Create virtual subsets of large rasters
//!
//! # Features
//!
//! - `std` (default) - Enable standard library support
//! - `async` - Enable async I/O support
//!
//! # Examples
//!
//! ## Create a Simple VRT Mosaic
//!
//! ```rust,no_run
//! use oxigdal_vrt::VrtBuilder;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let vrt = VrtBuilder::new()
//!     .add_tile("/data/tile1.tif", 0, 0, 512, 512)?
//!     .add_tile("/data/tile2.tif", 512, 0, 512, 512)?
//!     .add_tile("/data/tile3.tif", 0, 512, 512, 512)?
//!     .add_tile("/data/tile4.tif", 512, 512, 512, 512)?
//!     .build_file("mosaic.vrt")?;
//!
//! println!("Created VRT: {}x{}", vrt.raster_x_size, vrt.raster_y_size);
//! # Ok(())
//! # }
//! ```
//!
//! ## Read from a VRT
//!
//! ```rust,no_run
//! use oxigdal_vrt::VrtReader;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let reader = VrtReader::open("mosaic.vrt")?;
//! println!("VRT dimensions: {}x{}", reader.width(), reader.height());
//! println!("Bands: {}", reader.band_count());
//!
//! // Read a band (lazy evaluation - only reads from source files as needed)
//! let band_data = reader.read_band(1)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Create a Multi-Band VRT
//!
//! ```rust,no_run
//! use oxigdal_vrt::{VrtBuilder, VrtBand, VrtSource, SourceFilename};
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut builder = VrtBuilder::with_size(1024, 1024);
//!
//! // Band 1: Red
//! let red_source = VrtSource::new(SourceFilename::absolute("/data/red.tif"), 1);
//! let red_band = VrtBand::simple(1, RasterDataType::UInt8, red_source);
//! builder = builder.add_band(red_band)?;
//!
//! // Band 2: Green
//! let green_source = VrtSource::new(SourceFilename::absolute("/data/green.tif"), 1);
//! let green_band = VrtBand::simple(2, RasterDataType::UInt8, green_source);
//! builder = builder.add_band(green_band)?;
//!
//! // Band 3: Blue
//! let blue_source = VrtSource::new(SourceFilename::absolute("/data/blue.tif"), 1);
//! let blue_band = VrtBand::simple(3, RasterDataType::UInt8, blue_source);
//! builder = builder.add_band(blue_band)?;
//!
//! let vrt = builder.build_file("rgb.vrt")?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Use Mosaic Builder for Grid Layout
//!
//! ```rust,no_run
//! use oxigdal_vrt::MosaicBuilder;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mosaic = MosaicBuilder::new(256, 256)
//!     .add_tile("/tile_0_0.tif")?
//!     .next_column()
//!     .add_tile("/tile_1_0.tif")?
//!     .next_row()
//!     .add_tile("/tile_0_1.tif")?
//!     .next_column()
//!     .add_tile("/tile_1_1.tif")?
//!     .with_srs("EPSG:4326")
//!     .build_file("grid.vrt")?;
//! # Ok(())
//! # }
//! ```
//!
//! # Architecture
//!
//! The VRT driver is organized into several modules:
//!
//! - [`error`] - Error types for VRT operations
//! - [`source`] - Source raster references and windowing
//! - [`band`] - Virtual band configuration
//! - [`dataset`] - VRT dataset definition
//! - [`xml`] - XML parser and writer
//! - [`builder`] - Fluent builder API
//! - [`reader`] - Lazy reader with caching
//! - [`mosaic`] - Mosaicking and compositing logic

#![warn(missing_docs)]
#![warn(clippy::all)]
// Pedantic disabled to reduce noise - default clippy::all is sufficient
// #![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::expect_used))]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]

pub mod band;
pub mod builder;
pub mod dataset;
pub mod error;
pub mod mosaic;
pub mod reader;
pub mod source;
pub mod xml;

// Re-export commonly used types
pub use band::{ColorEntry, ColorTable, PixelFunction, VrtBand};
pub use builder::{MosaicBuilder, VrtBuilder};
pub use dataset::{VrtDataset, VrtMetadata, VrtSubclass};
pub use error::{Result, VrtError};
pub use mosaic::{BlendMode, CompositeParams, MosaicCompositor, MosaicPlanner};
pub use reader::{SourceDataset, VrtReader};
pub use source::{PixelRect, SourceFilename, SourceProperties, SourceWindow, VrtSource};
pub use xml::{VrtXmlParser, VrtXmlWriter};

/// Checks if data looks like a VRT file
///
/// # Examples
///
/// ```
/// use oxigdal_vrt::is_vrt;
///
/// let vrt_data = b"<VRTDataset rasterXSize=\"512\" rasterYSize=\"512\">";
/// assert!(is_vrt(vrt_data));
///
/// let not_vrt = b"GIF89a";
/// assert!(!is_vrt(not_vrt));
/// ```
#[must_use]
pub fn is_vrt(data: &[u8]) -> bool {
    if data.len() < 12 {
        return false;
    }

    // Check for XML declaration or VRTDataset tag
    let start = std::str::from_utf8(&data[..200.min(data.len())]).unwrap_or("");
    start.contains("<?xml") && start.contains("<VRTDataset") || start.starts_with("<VRTDataset")
}

/// VRT driver version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// VRT driver name
pub const DRIVER_NAME: &str = "VRT";

/// VRT driver description
pub const DRIVER_DESCRIPTION: &str = "Virtual Raster";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_vrt() {
        let vrt_xml =
            b"<?xml version=\"1.0\"?>\n<VRTDataset rasterXSize=\"512\" rasterYSize=\"512\">";
        assert!(is_vrt(vrt_xml));

        let vrt_no_decl = b"<VRTDataset rasterXSize=\"512\" rasterYSize=\"512\">";
        assert!(is_vrt(vrt_no_decl));

        let not_vrt = b"GIF89a";
        assert!(!is_vrt(not_vrt));

        let tiff = b"\x49\x49\x2A\x00";
        assert!(!is_vrt(tiff));
    }

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(DRIVER_NAME, "VRT");
    }
}

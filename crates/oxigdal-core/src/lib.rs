//! `OxiGDAL` Core - Pure Rust Geospatial Abstractions
//!
//! This crate provides the core types and traits for the `OxiGDAL` ecosystem,
//! a pure Rust reimplementation of GDAL for cloud-native geospatial computing.
//!
//! # Features
//!
//! - `std` (default) - Enable standard library support
//! - `alloc` - Enable allocation support without full std
//! - `arrow` - Enable Apache Arrow integration for zero-copy buffers
//! - `async` - Enable async I/O traits
//!
//! # Core Types
//!
//! - [`BoundingBox`] - 2D spatial extent
//! - [`GeoTransform`] - Affine transformation for georeferencing
//! - [`RasterDataType`] - Pixel data types
//! - [`buffer::RasterBuffer`] - Typed raster data buffer
//!
//! # Example
//!
//! ```
//! use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::error::Result;
//!
//! # fn main() -> Result<()> {
//! // Create a bounding box
//! let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)?;
//!
//! // Create a geotransform for a 1-degree resolution grid
//! let gt = GeoTransform::from_bounds(&bbox, 360, 180)?;
//!
//! // Create a raster buffer
//! let buffer = RasterBuffer::zeros(360, 180, RasterDataType::Float32);
//! # Ok(())
//! # }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![warn(clippy::all)]
// Pedantic disabled to reduce noise - default clippy::all is sufficient
// #![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod buffer;
pub mod error;
pub mod io;
pub mod memory;
pub mod simd_buffer;
pub mod types;
pub mod vector;

// Tutorial documentation
pub mod tutorials;

// Re-export commonly used items
pub use error::{OxiGdalError, Result};
pub use types::{BoundingBox, GeoTransform, RasterDataType, RasterMetadata};

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate name
pub const NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(NAME, "oxigdal-core");
    }
}

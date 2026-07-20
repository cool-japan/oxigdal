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

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

/// Internal prelude that makes `alloc`-provided types and macros available in
/// `no_std` builds.
///
/// Under the `std` feature these names come from the standard prelude, so this
/// module is only wired in for `no_std` (`#[cfg(not(feature = "std"))]`) to keep
/// the `std` build byte-for-byte identical. Modules that use bare `Vec`,
/// `String`, `Box`, `format!` or `vec!` add
/// `#[cfg(not(feature = "std"))] use crate::compat::*;` at their top.
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
pub(crate) mod compat {
    pub use alloc::borrow::ToOwned;
    pub use alloc::boxed::Box;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec::Vec;
    pub use alloc::{format, vec};
}

pub mod buffer;
pub mod error;
pub mod io;
// The advanced memory-management module (custom allocators, memory-mapped I/O,
// NUMA/huge-page support) relies on `parking_lot`, hashed collections, the global
// allocator and OS primitives, so it requires the standard library.
#[cfg(feature = "std")]
pub mod memory;
pub mod simd_buffer;
pub mod types;
pub mod vector;

// Tutorial documentation
pub mod tutorials;

// Re-export commonly used items
pub use error::{OxiGdalError, Result};
#[cfg(feature = "std")]
pub use io::{Dataset, FieldType, RasterDataset, VectorDataset};
pub use types::{
    BoundingBox, ColorEntry, ColorTable, ColorTableKind, CrsFormat, GeoTransform, Histogram,
    RasterDataType, RasterMetadata, SpatialReference, Statistics,
};
pub use vector::FieldValue;

pub use buffer::Mask;
#[cfg(feature = "std")]
pub use io::{MmapDataSource, MmapDataSourceRw};

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

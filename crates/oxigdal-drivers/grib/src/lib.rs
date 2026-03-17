//! OxiGDAL GRIB Driver - Pure Rust GRIB1/GRIB2 Meteorological Data Format Support
//!
//! This crate provides comprehensive support for reading GRIB (GRIdded Binary) format files,
//! which are commonly used for meteorological and climate data. Both GRIB Edition 1 and
//! GRIB Edition 2 formats are supported.
//!
//! # Features
//!
//! - **Pure Rust Implementation**: No C/Fortran dependencies, fully compliant with COOLJAPAN policies
//! - **GRIB1 and GRIB2 Support**: Read both legacy and modern GRIB formats
//! - **Parameter Tables**: WMO standard parameter lookups for meteorological variables
//! - **Grid Definitions**: Support for regular lat/lon, Lambert conformal, Mercator, polar stereographic
//! - **Data Decoding**: Efficient unpacking of packed binary data with proper scaling
//! - **Integration**: Designed to integrate seamlessly with oxigdal-core Dataset
//!
//! # Quick Start
//!
//! ```no_run
//! use oxigdal_grib::reader::GribReader;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Open a GRIB file
//! let mut reader = GribReader::open("data/forecast.grib2")?;
//!
//! // Read all messages
//! for record in reader {
//!     let record = record?;
//!
//!     // Get parameter information
//!     let param = record.parameter()?;
//!     println!("Parameter: {} ({})", param.long_name, param.units);
//!
//!     // Get level information
//!     println!("Level: {:?} = {}", record.level_type(), record.level_value());
//!
//!     // Decode data
//!     let data = record.decode_data()?;
//!     println!("Data points: {}", data.len());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # GRIB Format Overview
//!
//! GRIB (GRIdded Binary) is a concise data format commonly used in meteorology for storing
//! historical and forecast weather data. It is standardized by the World Meteorological
//! Organization (WMO).
//!
//! ## GRIB1 Structure
//!
//! - Indicator Section (IS): Magic bytes 'GRIB' and edition number
//! - Product Definition Section (PDS): Metadata about the parameter, level, time
//! - Grid Definition Section (GDS): Grid geometry and projection
//! - Bit Map Section (BMS): Optional bitmap for missing values
//! - Binary Data Section (BDS): Packed binary data
//! - End Section: Magic bytes '7777'
//!
//! ## GRIB2 Structure
//!
//! - Section 0: Indicator Section
//! - Section 1: Identification Section
//! - Section 2: Local Use Section (optional)
//! - Section 3: Grid Definition Section
//! - Section 4: Product Definition Section
//! - Section 5: Data Representation Section
//! - Section 6: Bit-Map Section (optional)
//! - Section 7: Data Section
//! - Section 8: End Section
//!
//! # Architecture
//!
//! The crate is organized into several modules:
//!
//! - [`error`]: Error types for GRIB operations
//! - [`message`]: Core message parsing and iteration
//! - [`parameter`]: WMO parameter tables and lookups
//! - [`grid`]: Grid definitions and coordinate systems
//! - [`grib1`]: GRIB Edition 1 format support
//! - [`grib2`]: GRIB Edition 2 format support
//! - [`reader`]: High-level file reading API
//!
//! # Performance
//!
//! This implementation focuses on correctness and simplicity while maintaining reasonable
//! performance. For production use with large GRIB files, consider:
//!
//! - Using buffered I/O (automatically done with `GribReader::open`)
//! - Processing messages in parallel when appropriate
//! - Caching decoded data if the same message is accessed multiple times

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

#[cfg(not(feature = "std"))]
extern crate alloc;

pub mod error;
pub mod grid;
pub mod message;
pub mod parameter;
pub mod reader;
pub mod templates;

#[cfg(feature = "grib1")]
pub mod grib1;

#[cfg(feature = "grib2")]
pub mod grib2;

// Re-exports for convenience
pub use error::{GribError, Result};
pub use grid::GridDefinition;
pub use message::{GribEdition, GribMessage};
pub use parameter::{LevelType, Parameter};
pub use reader::{GribReader, GribRecord};

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Check if GRIB1 support is enabled
pub const fn has_grib1_support() -> bool {
    cfg!(feature = "grib1")
}

/// Check if GRIB2 support is enabled
pub const fn has_grib2_support() -> bool {
    cfg!(feature = "grib2")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_feature_flags() {
        // Both should be enabled by default
        assert!(has_grib1_support());
        assert!(has_grib2_support());
    }
}

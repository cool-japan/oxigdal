//! OxiGDAL NetCDF Driver - Pure Rust NetCDF-3 with Optional NetCDF-4 Support
//!
//! This crate provides NetCDF file format support for OxiGDAL, following the
//! COOLJAPAN Pure Rust policy.
//!
//! # Pure Rust Policy Compliance
//!
//! **IMPORTANT**: This driver provides the structure and API for Pure Rust NetCDF support,
//! but the actual netcdf3 integration is currently incomplete due to breaking API changes
//! in netcdf3 v0.6.0. The driver demonstrates:
//!
//! - Complete Pure Rust data structures for NetCDF metadata (dimensions, variables, attributes)
//! - CF conventions support
//! - Feature-gated architecture for Pure Rust vs. C-binding implementations
//!
//! **Status**: The reader/writer implementations need to be updated to use the new
//! `Dataset`/`FileReader`/`FileWriter` API from netcdf3 v0.6.0 (breaking change from v0.1.0).
//!
//! For NetCDF-4 (HDF5-based) support, you can enable the `netcdf4` feature,
//! which requires system libraries (libnetcdf, libhdf5) and is **NOT Pure Rust**.
//!
//! ## Feature Flags
//!
//! - `netcdf3` (default): Pure Rust NetCDF-3 support via netcdf3 crate
//! - `netcdf4`: NetCDF-4/HDF5 support via C bindings (requires system libraries)
//! - `cf_conventions`: CF (Climate and Forecast) conventions support
//! - `async`: Async I/O support
//! - `compression`: Compression support (NetCDF-4 only)
//!
//! # NetCDF Format Support
//!
//! ## NetCDF-3 (Pure Rust, Default)
//!
//! Fully supported data types:
//! - `i8`, `i16`, `i32` - Signed integers
//! - `f32`, `f64` - Floating point numbers
//! - `char` - Character data
//!
//! Features:
//! - Fixed and unlimited dimensions
//! - Multi-dimensional arrays
//! - Variable and global attributes
//! - Coordinate variables
//!
//! ## NetCDF-4 (C Bindings, Feature-Gated)
//!
//! Additional data types (requires `netcdf4` feature):
//! - `u8`, `u16`, `u32`, `u64` - Unsigned integers
//! - `i64`, `u64` - 64-bit integers
//! - `string` - Variable-length strings
//!
//! Additional features (requires `netcdf4` feature):
//! - HDF5-based compression
//! - Groups and nested groups
//! - User-defined types
//! - Multiple unlimited dimensions
//!
//! # Example - Reading NetCDF-3 File (Pure Rust)
//!
//! ```ignore
//! use oxigdal_netcdf::NetCdfReader;
//!
//! // Open a NetCDF-3 file
//! let reader = NetCdfReader::open("data.nc")?;
//!
//! // Get metadata
//! println!("{}", reader.metadata().summary());
//!
//! // List dimensions
//! for dim in reader.dimensions().iter() {
//!     println!("Dimension: {} (size: {})", dim.name(), dim.len());
//! }
//!
//! // List variables
//! for var in reader.variables().iter() {
//!     println!("Variable: {} (type: {})", var.name(), var.data_type().name());
//! }
//!
//! // Read variable data
//! let temperature = reader.read_f32("temperature")?;
//! println!("Temperature data: {:?}", temperature);
//! ```
//!
//! # Example - Writing NetCDF-3 File (Pure Rust)
//!
//! ```ignore
//! use oxigdal_netcdf::{NetCdfWriter, NetCdfVersion};
//! use oxigdal_netcdf::dimension::Dimension;
//! use oxigdal_netcdf::variable::{Variable, DataType};
//! use oxigdal_netcdf::attribute::{Attribute, AttributeValue};
//!
//! // Create a new NetCDF-3 file
//! let mut writer = NetCdfWriter::create("output.nc", NetCdfVersion::Classic)?;
//!
//! // Add dimensions
//! writer.add_dimension(Dimension::new_unlimited("time", 0)?)?;
//! writer.add_dimension(Dimension::new("lat", 180)?)?;
//! writer.add_dimension(Dimension::new("lon", 360)?)?;
//!
//! // Add coordinate variables
//! writer.add_variable(Variable::new_coordinate("time", DataType::F64)?)?;
//! writer.add_variable(Variable::new_coordinate("lat", DataType::F32)?)?;
//! writer.add_variable(Variable::new_coordinate("lon", DataType::F32)?)?;
//!
//! // Add data variable
//! let temp_var = Variable::new(
//!     "temperature",
//!     DataType::F32,
//!     vec!["time".to_string(), "lat".to_string(), "lon".to_string()],
//! )?;
//! writer.add_variable(temp_var)?;
//!
//! // Add variable attributes
//! writer.add_variable_attribute(
//!     "temperature",
//!     Attribute::new("units", AttributeValue::text("celsius"))?,
//! )?;
//! writer.add_variable_attribute(
//!     "temperature",
//!     Attribute::new("long_name", AttributeValue::text("Air Temperature"))?,
//! )?;
//!
//! // Add global attributes
//! writer.add_global_attribute(
//!     Attribute::new("Conventions", AttributeValue::text("CF-1.8"))?,
//! )?;
//! writer.add_global_attribute(
//!     Attribute::new("title", AttributeValue::text("Temperature Data"))?,
//! )?;
//!
//! // End define mode
//! writer.end_define_mode()?;
//!
//! // Write data
//! let time_data = vec![0.0, 1.0, 2.0];
//! writer.write_f64("time", &time_data)?;
//!
//! let lat_data: Vec<f32> = (0..180).map(|i| -90.0 + i as f32).collect();
//! writer.write_f32("lat", &lat_data)?;
//!
//! let lon_data: Vec<f32> = (0..360).map(|i| -180.0 + i as f32).collect();
//! writer.write_f32("lon", &lon_data)?;
//!
//! // Write temperature data
//! let temp_data = vec![20.0f32; 3 * 180 * 360];
//! writer.write_f32("temperature", &temp_data)?;
//!
//! // Close file
//! writer.close()?;
//! ```
//!
//! # CF Conventions Support
//!
//! The driver recognizes and parses CF (Climate and Forecast) conventions metadata:
//!
//! ```ignore
//! use oxigdal_netcdf::NetCdfReader;
//!
//! let reader = NetCdfReader::open("cf_data.nc")?;
//!
//! if let Some(cf) = reader.cf_metadata() {
//!     if cf.is_cf_compliant() {
//!         println!("CF Conventions: {}", cf.conventions.as_deref().unwrap_or(""));
//!         println!("Title: {}", cf.title.as_deref().unwrap_or(""));
//!         println!("Institution: {}", cf.institution.as_deref().unwrap_or(""));
//!     }
//! }
//! ```
//!
//! # Pure Rust Limitations
//!
//! When using the default Pure Rust mode (NetCDF-3 only):
//!
//! - No NetCDF-4/HDF5 format support
//! - No compression support
//! - No groups or user-defined types
//! - Only one unlimited dimension allowed
//! - Limited to NetCDF-3 data types
//!
//! To use NetCDF-4 features, enable the `netcdf4` feature (requires C dependencies):
//!
//! ```toml
//! [dependencies]
//! oxigdal-netcdf = { version = "0.1", features = ["netcdf4"] }
//! ```
//!
//! **Note**: Enabling `netcdf4` violates the COOLJAPAN Pure Rust policy and requires
//! system libraries (libnetcdf ≥ 4.0, libhdf5 ≥ 1.8).
//!
//! # Performance Considerations
//!
//! - Pure Rust NetCDF-3 reader/writer has comparable performance to C libraries
//! - For large datasets, consider using chunked reading/writing
//! - Unlimited dimensions may have performance implications
//! - CF metadata parsing is done on-demand
//!
//! # References
//!
//! - [NetCDF User Guide](https://www.unidata.ucar.edu/software/netcdf/docs/)
//! - [CF Conventions](http://cfconventions.org/)
//! - [netcdf3 crate](https://crates.io/crates/netcdf3)

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(clippy::all)]
// Pedantic disabled to reduce noise - default clippy::all is sufficient
// #![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
// Allow unexpected cfg for optional netcdf4 feature
#![allow(unexpected_cfgs)]
// Allow unused imports during development
#![allow(unused_imports)]
// Allow missing docs during API development
#![allow(missing_docs)]
// Allow dead code for future netcdf3/netcdf4 integration
#![allow(dead_code)]
// Allow manual div_ceil for dimension calculations
#![allow(clippy::manual_div_ceil)]
// Allow expect() for internal netcdf state invariants
#![allow(clippy::expect_used)]
// Allow collapsible match for netcdf error handling
#![allow(clippy::collapsible_match)]
// Allow struct field pub visibility in internal modules
#![allow(clippy::redundant_field_names)]

#[cfg(feature = "std")]
extern crate std;

pub mod attribute;
#[cfg(feature = "cf_conventions")]
pub mod cf_conventions;
pub mod dimension;
pub mod error;
pub mod metadata;
#[cfg(feature = "netcdf3")]
pub(crate) mod nc3_compat;
pub mod netcdf4;
pub mod reader;
pub mod variable;
pub mod writer;

// Re-export commonly used types
pub use attribute::{Attribute, AttributeValue, Attributes};
pub use dimension::{Dimension, DimensionSize, Dimensions};
pub use error::{NetCdfError, Result};
pub use metadata::{CfMetadata, NetCdfMetadata, NetCdfVersion};
pub use netcdf4::{
    ChunkInfo, CompressionFilter, Hdf5ByteOrder, Hdf5DatatypeClass, Hdf5MessageType,
    Hdf5Superblock, Hdf5SuperblockVersion, Nc4Group, Nc4Reader, Nc4VariableInfo, Nc4Writer,
};
pub use reader::NetCdfReader;
pub use variable::{DataType, Variable, Variables};
pub use writer::NetCdfWriter;

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate name
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// Pure Rust compliance status
///
/// Returns true if running in Pure Rust mode (no C dependencies).
/// Returns false if netcdf4 feature is enabled (requires C libraries).
#[must_use]
pub const fn is_pure_rust() -> bool {
    !cfg!(feature = "netcdf4")
}

/// Check if NetCDF-3 support is available.
#[must_use]
pub const fn has_netcdf3() -> bool {
    cfg!(feature = "netcdf3")
}

/// Check if NetCDF-4 support is available.
#[must_use]
pub const fn has_netcdf4() -> bool {
    cfg!(feature = "netcdf4")
}

/// Get supported format versions.
#[must_use]
#[allow(unused_mut)]
pub fn supported_versions() -> Vec<NetCdfVersion> {
    let mut versions = Vec::new();

    #[cfg(feature = "netcdf3")]
    {
        versions.push(NetCdfVersion::Classic);
        versions.push(NetCdfVersion::Offset64Bit);
    }

    #[cfg(feature = "netcdf4")]
    {
        versions.push(NetCdfVersion::NetCdf4);
        versions.push(NetCdfVersion::NetCdf4Classic);
    }

    versions
}

/// Get driver information.
#[must_use]
pub fn info() -> String {
    let pure_rust = if is_pure_rust() {
        "Pure Rust"
    } else {
        "C Bindings"
    };

    let versions: Vec<&str> = supported_versions()
        .iter()
        .map(|v| v.format_name())
        .collect();

    format!(
        "{} {} - {} - Supports: {}",
        NAME,
        VERSION,
        pure_rust,
        versions.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(NAME, "oxigdal-netcdf");
    }

    #[test]
    fn test_pure_rust_status() {
        #[cfg(feature = "netcdf4")]
        assert!(!is_pure_rust());

        #[cfg(not(feature = "netcdf4"))]
        assert!(is_pure_rust());
    }

    #[test]
    fn test_feature_detection() {
        #[cfg(feature = "netcdf3")]
        assert!(has_netcdf3());

        #[cfg(feature = "netcdf4")]
        assert!(has_netcdf4());
    }

    #[test]
    fn test_supported_versions() {
        let versions = supported_versions();

        // When no features enabled, versions list is empty
        #[cfg(all(not(feature = "netcdf3"), not(feature = "netcdf4")))]
        assert!(versions.is_empty());

        #[cfg(any(feature = "netcdf3", feature = "netcdf4"))]
        assert!(!versions.is_empty());

        #[cfg(feature = "netcdf3")]
        assert!(versions.contains(&NetCdfVersion::Classic));
    }

    #[test]
    fn test_info() {
        let info = info();
        assert!(info.contains(NAME));
        assert!(info.contains(VERSION));
    }
}

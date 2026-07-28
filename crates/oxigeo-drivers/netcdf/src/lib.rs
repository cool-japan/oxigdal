//! OxiGeo NetCDF Driver - Pure Rust NetCDF-4 (HDF5) with Optional NetCDF-3
//!
//! This crate provides NetCDF file format support for OxiGeo, following the
//! COOLJAPAN Pure Rust policy.
//!
//! # Pure Rust Policy Compliance
//!
//! Real NetCDF-4 files (which are HDF5 files carrying the NetCDF-4 conventions)
//! are read and written with the Pure-Rust [`oxinetcdf`] crate atop
//! [`oxih5`](https://crates.io/crates/oxih5). There is **no** `libnetcdf`, no
//! `libhdf5`, and no FFI — the default build is 100% Pure Rust.
//!
//! - Reading honours the NetCDF-4 conventions: dimension scales, coordinate
//!   variables, `DIMENSION_LIST` axis linkage, sub-groups (flattened into
//!   `"<group>/<var>"` names, recursively), and user attributes (`units`,
//!   `_FillValue`, `scale_factor`, `add_offset`, …) — surfaced via
//!   [`Variable::attributes`]. `read_f32`/`read_f64`/`read_i32` are raw,
//!   unprocessed reads; `_FillValue`/`scale_factor`/`add_offset` are applied
//!   only through the explicit opt-in [`NetCdfReader::read_f64_cf`] /
//!   [`NetCdfReader::read_f32_cf`] (CF §8.1 packed-data unpacking + §2.5.1
//!   fill-value masking to `NaN`).
//! - Writing produces real HDF5/NetCDF-4 files via the Pure-Rust backend.
//! - Optional NetCDF-3 classic support is available behind the `netcdf3`
//!   feature (the `netcdf3` crate, also Pure Rust).
//!
//! ## Feature Flags
//!
//! - `std` (default): standard-library support.
//! - `netcdf3`: Pure Rust NetCDF-3 (classic / 64-bit offset) support via the
//!   `netcdf3` crate. NetCDF-4 support is always available and needs no feature.
//! - `cf_conventions`: CF (Climate and Forecast) conventions support
//! - `async`: Async I/O support
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
//! ## NetCDF-4 (Pure Rust, always available)
//!
//! Real NetCDF-4 / HDF5 files are read and written via the Pure-Rust
//! [`oxinetcdf`] backend. Additional data types over NetCDF-3:
//! - `u8`, `u16`, `u32`, `u64` - Unsigned integers
//! - `i64`, `u64` - 64-bit integers
//! - `string` - Variable-length strings
//!
//! Additional features:
//! - HDF5-based (DEFLATE) compression
//! - Groups and coordinate variables
//! - Multiple unlimited dimensions
//!
//! # Example - Reading NetCDF-3 File (Pure Rust)
//!
//! ```ignore
//! use oxigeo_netcdf::NetCdfReader;
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
//! use oxigeo_netcdf::{NetCdfWriter, NetCdfVersion};
//! use oxigeo_netcdf::dimension::Dimension;
//! use oxigeo_netcdf::variable::{Variable, DataType};
//! use oxigeo_netcdf::attribute::{Attribute, AttributeValue};
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
//! use oxigeo_netcdf::NetCdfReader;
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
//! # Pure Rust Notes
//!
//! - NetCDF-4 reading/writing is Pure Rust via [`oxinetcdf`] atop `oxih5`
//!   (DEFLATE compression through `oxiarc-deflate`); no C libraries are used.
//! - The Pure-Rust NetCDF-4 writer supports data variables, dimensions, and
//!   string attributes. Constructs it cannot yet represent (e.g. explicit
//!   coordinate-variable values or numeric attributes) return a typed error
//!   rather than producing an incomplete file.
//! - NetCDF-3 classic support is optional (`netcdf3` feature) and allows only
//!   one unlimited dimension per the classic model.
//!
//! # Performance Considerations
//!
//! - For large datasets, consider using chunked reading/writing
//! - CF metadata parsing is done on-demand
//!
//! # References
//!
//! - [NetCDF User Guide](https://www.unidata.ucar.edu/software/netcdf/docs/)
//! - [CF Conventions](http://cfconventions.org/)
//! - [oxinetcdf crate](https://crates.io/crates/oxinetcdf)

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
/// A **dead, unused, hand-rolled** HDF5/NetCDF-4 parser/writer, kept for
/// backward source compatibility only.
///
/// This module is a completely separate implementation from the crate's real
/// NetCDF-4 backend ([`reader::NetCdfReader`] / [`writer::NetCdfWriter`],
/// backed by the Pure-Rust [`oxinetcdf`] crate atop `oxih5`) — nothing in
/// `reader`/`writer` calls into it. Its `Nc4Reader::open` unconditionally
/// returns [`error::NetCdfError::NetCdf4NotAvailable`] (root-group object
/// header parsing was never finished), so it can never successfully read a
/// file. **Do not use `Nc4Reader`/`Nc4Writer` for real work** — use
/// [`reader::NetCdfReader`] / [`writer::NetCdfWriter`] instead, which are
/// real, tested, and exercised by this crate's own round-trip tests.
///
/// These types are intentionally **not** re-exported at the crate root
/// (unlike `reader`/`writer`'s types) so that a normal `use oxigeo_netcdf::*`
/// import surface can't accidentally reach for the non-functional
/// `Nc4Reader`/`Nc4Writer` instead of the real backend. Reach them explicitly
/// via `oxigeo_netcdf::netcdf4::{Nc4Reader, Nc4Writer, ...}` if you must.
pub mod netcdf4;
pub mod reader;
pub mod variable;
pub mod writer;

// Re-export commonly used types.
//
// `netcdf4`'s types (`Nc4Reader`/`Nc4Writer`/...) are deliberately NOT
// re-exported here — see the `netcdf4` module doc for why.
pub use attribute::{Attribute, AttributeValue, Attributes};
pub use dimension::{Dimension, DimensionSize, Dimensions};
pub use error::{NetCdfError, Result};
pub use metadata::{CfMetadata, NetCdfMetadata, NetCdfVersion};
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
///
/// Always `true`: NetCDF-4 (HDF5) reading/writing is provided by the Pure-Rust
/// [`oxinetcdf`] backend and needs no feature flag.
#[must_use]
pub const fn has_netcdf4() -> bool {
    true
}

/// Get supported format versions.
///
/// NetCDF-4 variants are always supported (Pure-Rust `oxinetcdf` backend);
/// NetCDF-3 variants require the optional `netcdf3` feature.
#[must_use]
#[allow(unused_mut)]
pub fn supported_versions() -> Vec<NetCdfVersion> {
    // NetCDF-4 is always available via the Pure-Rust backend.
    let mut versions = vec![NetCdfVersion::NetCdf4, NetCdfVersion::NetCdf4Classic];

    #[cfg(feature = "netcdf3")]
    {
        versions.push(NetCdfVersion::Classic);
        versions.push(NetCdfVersion::Offset64Bit);
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
        assert_eq!(NAME, "oxigeo-netcdf");
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

        // NetCDF-4 is always available via the Pure-Rust oxinetcdf backend.
        assert!(!versions.is_empty());
        assert!(versions.contains(&NetCdfVersion::NetCdf4));

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

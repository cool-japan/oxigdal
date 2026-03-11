//! OxiGDAL HDF5 Driver - Pure Rust HDF5 with Optional C-Binding Support
//!
//! This crate provides HDF5 file format support for OxiGDAL, following the
//! COOLJAPAN Pure Rust policy.
//!
//! # Pure Rust Policy Compliance
//!
//! **IMPORTANT**: This driver provides Pure Rust minimal HDF5 support by default,
//! with feature-gated C bindings for full HDF5 functionality.
//!
//! ## Default: Pure Rust Minimal HDF5
//!
//! The default implementation provides basic HDF5 reading and writing capabilities
//! in Pure Rust, inspired by the [hdf5file](https://github.com/sile/hdf5file)
//! and [oxifive](https://github.com/dragly/oxifive) projects.
//!
//! **Supported Features (Pure Rust):**
//! - Reading and writing HDF5 1.0 format (Superblock Version 0)
//! - Basic datatypes: i8, u8, i16, u16, i32, u32, i64, u64, f32, f64
//! - Fixed-length strings
//! - Multi-dimensional datasets
//! - Hierarchical groups
//! - Attributes (metadata)
//! - GZIP compression (via Pure Rust flate2)
//! - Contiguous and chunked layouts
//!
//! **Limitations (Pure Rust):**
//! - No HDF5 2.0/3.0 features (requires hdf5_sys)
//! - No compound types (requires hdf5_sys)
//! - No variable-length types (requires hdf5_sys)
//! - No SZIP compression (requires hdf5_sys)
//! - No advanced filters (requires hdf5_sys)
//! - Limited to simple use cases
//!
//! ## Full HDF5 Support (C Bindings)
//!
//! For full HDF5 functionality, enable the `hdf5_sys` feature, which uses the
//! [hdf5-rust](https://github.com/aldanor/hdf5-rust) crate with C bindings.
//!
//! **Warning**: Enabling `hdf5_sys` requires system libhdf5 library and is
//! **NOT Pure Rust**.
//!
//! ## Feature Flags
//!
//! - `std` (default): Standard library support
//! - `pure_rust` (default): Pure Rust minimal HDF5 implementation
//! - `hdf5_sys`: Full HDF5 support via C bindings (NOT Pure Rust)
//! - `compression`: Compression support (GZIP in Pure Rust, all filters with hdf5_sys)
//! - `async`: Async I/O support
//!
//! # HDF5 Format Overview
//!
//! HDF5 (Hierarchical Data Format version 5) is a file format designed for
//! storing and organizing large amounts of data. It's widely used in scientific
//! computing, particularly for:
//!
//! - Climate and weather data (NetCDF-4 is built on HDF5)
//! - Satellite imagery (HDF-EOS)
//! - Astronomy datasets
//! - Medical imaging
//! - Machine learning model storage
//!
//! ## Key Concepts
//!
//! - **File**: Container for all HDF5 data
//! - **Group**: Directory-like container for organizing objects
//! - **Dataset**: Multi-dimensional array of homogeneous data
//! - **Attribute**: Small metadata attached to groups or datasets
//! - **Datatype**: Description of data element type
//! - **Dataspace**: Description of dataset dimensions
//!
//! # Example - Writing HDF5 File (Pure Rust)
//!
//! ```ignore
//! use oxigdal_hdf5::{Hdf5Writer, Hdf5Version};
//! use oxigdal_hdf5::datatype::Datatype;
//! use oxigdal_hdf5::dataset::DatasetProperties;
//! use oxigdal_hdf5::attribute::Attribute;
//!
//! // Create HDF5 file
//! let mut writer = Hdf5Writer::create("output.h5", Hdf5Version::V10)?;
//!
//! // Create group
//! writer.create_group("/measurements")?;
//!
//! // Add group attribute
//! writer.add_group_attribute(
//!     "/measurements",
//!     Attribute::string("description", "Temperature measurements")
//! )?;
//!
//! // Create dataset
//! writer.create_dataset(
//!     "/measurements/temperature",
//!     Datatype::Float32,
//!     vec![100, 200],  // 100x200 array
//!     DatasetProperties::new()
//! )?;
//!
//! // Write data
//! let data: Vec<f32> = vec![20.5; 20000];  // 100 * 200 elements
//! writer.write_f32("/measurements/temperature", &data)?;
//!
//! // Add dataset attribute
//! writer.add_dataset_attribute(
//!     "/measurements/temperature",
//!     Attribute::string("units", "celsius")
//! )?;
//!
//! // Finalize file
//! writer.finalize()?;
//! # Ok::<(), oxigdal_hdf5::error::Hdf5Error>(())
//! ```
//!
//! # Example - Reading HDF5 File (Pure Rust)
//!
//! ```ignore
//! use oxigdal_hdf5::Hdf5Reader;
//!
//! // Open HDF5 file
//! let mut reader = Hdf5Reader::open("output.h5")?;
//!
//! // Get root group
//! let root = reader.root()?;
//! println!("Root group: {}", root.name());
//!
//! // List groups
//! for group_path in reader.list_groups() {
//!     println!("Group: {}", group_path);
//! }
//!
//! // List datasets
//! for dataset_path in reader.list_datasets() {
//!     let dataset = reader.dataset(dataset_path)?;
//!     println!("Dataset: {} (shape: {:?}, type: {})",
//!         dataset.name(),
//!         dataset.dims(),
//!         dataset.datatype()
//!     );
//! }
//!
//! // Read dataset
//! let temperature = reader.read_f32("/measurements/temperature")?;
//! println!("Temperature data: {} elements", temperature.len());
//! # Ok::<(), oxigdal_hdf5::error::Hdf5Error>(())
//! ```
//!
//! # Example - Chunked Dataset with Compression
//!
//! ```ignore
//! use oxigdal_hdf5::{Hdf5Writer, Hdf5Version};
//! use oxigdal_hdf5::datatype::Datatype;
//! use oxigdal_hdf5::dataset::{DatasetProperties, CompressionFilter};
//!
//! let mut writer = Hdf5Writer::create("compressed.h5", Hdf5Version::V10)?;
//!
//! // Create compressed dataset with chunking
//! let properties = DatasetProperties::new()
//!     .with_chunks(vec![10, 20])        // 10x20 chunks
//!     .with_gzip(6);                     // GZIP compression level 6
//!
//! writer.create_dataset(
//!     "/data",
//!     Datatype::Float64,
//!     vec![1000, 2000],
//!     properties
//! )?;
//!
//! // Write data
//! let data: Vec<f64> = vec![0.0; 2_000_000];
//! writer.write_f64("/data", &data)?;
//!
//! writer.finalize()?;
//! # Ok::<(), oxigdal_hdf5::error::Hdf5Error>(())
//! ```
//!
//! # Integration with OxiGDAL
//!
//! This driver integrates with the OxiGDAL core types and can be used to read
//! and write geospatial data in HDF5 format, particularly for:
//!
//! - HDF-EOS (Earth Observing System) satellite data
//! - NetCDF-4 files (which use HDF5 as backend)
//! - Climate model outputs
//! - Remote sensing imagery
//!
//! # References
//!
//! - [HDF5 Specification](https://portal.hdfgroup.org/display/HDF5/File+Format+Specification)
//! - [HDF5 User Guide](https://portal.hdfgroup.org/display/HDF5/HDF5+User+Guide)
//! - [hdf5file](https://github.com/sile/hdf5file) - Pure Rust HDF5 implementation
//! - [oxifive](https://github.com/dragly/oxifive) - Pure Rust HDF5 reader
//! - [hdf5-rust](https://github.com/aldanor/hdf5-rust) - HDF5 C bindings for Rust
//!
//! # See Also
//!
//! - [`oxigdal-netcdf`](../oxigdal_netcdf/index.html) - NetCDF driver (NetCDF-4 uses HDF5)
//! - [`oxigdal-geotiff`](../oxigdal_geotiff/index.html) - GeoTIFF driver
//! - [`oxigdal-zarr`](../oxigdal_zarr/index.html) - Zarr driver (alternative to HDF5)

#![warn(missing_docs)]
#![deny(unsafe_code)]

// Module declarations
pub mod attribute;
pub mod dataset;
pub mod datatype;
pub mod error;
pub mod filters;
pub mod group;
pub mod reader;
pub mod writer;

// Re-exports
pub use attribute::{Attribute, AttributeValue, Attributes};
pub use dataset::{CompressionFilter, Dataset, DatasetProperties, LayoutType};
pub use datatype::{
    CompoundMember, Datatype, DatatypeClass, EnumMember, Hdf5ByteOrder, StringPadding,
    TypeConverter,
};
pub use error::{Hdf5Error, Result};
pub use group::{Group, ObjectRef, ObjectType, PathUtils};
pub use reader::{Hdf5Reader, Hdf5ReaderBuilder, Superblock, SuperblockVersion};
pub use writer::{Hdf5Version, Hdf5Writer, Hdf5WriterBuilder};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Library name
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// Get library information
pub fn version_info() -> String {
    format!("{} v{}", NAME, VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_info() {
        let info = version_info();
        assert!(info.contains("oxigdal-hdf5"));
        assert!(info.contains(VERSION));
    }

    #[test]
    fn test_constants() {
        assert_eq!(NAME, "oxigdal-hdf5");
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    }
}

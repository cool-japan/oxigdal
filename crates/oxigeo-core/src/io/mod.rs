//! I/O abstractions for geospatial data access
//!
//! This module provides traits and implementations for reading and writing
//! geospatial data from various sources.
//!
//! # Features
//!
//! - [`DataSource`] - Synchronous data source trait
//! - `AsyncDataSource` - Asynchronous data source trait (requires `async` feature)
//! - [`ByteRange`] - Byte range specification for partial reads
//! - [`RasterRead`] / [`RasterWrite`] - Raster-specific I/O traits
//! - [`Dataset`] / [`RasterDataset`] / [`VectorDataset`] - High-level dataset traits

// The high-level dataset traits expose `Option<&std::path::Path>` in their API,
// so they require the standard library. The lower-level `traits` module
// (byte-range data sources, raster read/write) is `no_std`-compatible.
#[cfg(feature = "std")]
pub mod dataset;
mod traits;

#[cfg(feature = "std")]
pub use dataset::{Dataset, FieldType, RasterDataset, VectorDataset};

pub use traits::{
    ByteRange, CogSupport, DataSink, DataSource, OverviewSupport, RasterRead, RasterWrite,
};

#[cfg(feature = "async")]
pub use traits::{AsyncDataSource, AsyncRasterRead};

#[cfg(feature = "std")]
pub mod mmap;

#[cfg(feature = "std")]
pub use mmap::{MmapDataSource, MmapDataSourceRw};

#[cfg(feature = "std")]
mod file;

#[cfg(feature = "std")]
pub use file::FileDataSource;

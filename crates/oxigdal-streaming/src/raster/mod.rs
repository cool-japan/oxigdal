//! Raster data streaming for large geospatial datasets.
//!
//! This module provides async streaming capabilities for raster data,
//! enabling efficient processing of large datasets that don't fit in memory.

pub mod reader;
pub mod stream;
pub mod writer;

pub use reader::RasterStreamReader;
pub use stream::{RasterStream, RasterStreamConfig, RasterChunk};
pub use writer::RasterStreamWriter;

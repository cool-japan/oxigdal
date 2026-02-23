//! Raster operations for Python bindings
//!
//! This module provides comprehensive Python-friendly interfaces to raster operations
//! including I/O, calculator, warping, resampling, and metadata handling.

mod core_ops;
mod operations;
mod types;
mod warp_engine;

// Re-export public types used by lib.rs
pub use types::{RasterMetadataPy, WindowPy};

// Re-export public functions
pub use core_ops::{calc, create_raster, open_raster};
pub use operations::{
    build_overviews, clip, get_metadata, merge, read, read_bands, resample, translate, warp, write,
};

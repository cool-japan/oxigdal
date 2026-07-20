//! Algorithm bindings for Python
//!
//! This module provides Python bindings for raster processing algorithms
//! including statistics, filters, morphological operations, and spectral indices.

pub mod classify;
pub mod edges;
pub mod eviconfig_traits;
pub mod filters;
pub mod kmeans;
pub mod spectral;
pub mod stats;
pub mod types;

// Re-export the `#[pyfunction]` items so `crate::algorithms::*` (used by
// lib.rs) exposes them exactly as the pre-split single-file module did.
// `types`/`eviconfig_traits` hold internal helper types consumed directly by
// their sibling submodules (`use super::types::...`) and are not otherwise
// re-exported here.
pub use classify::*;
pub use edges::*;
pub use filters::*;
pub use kmeans::*;
pub use spectral::*;
pub use stats::*;

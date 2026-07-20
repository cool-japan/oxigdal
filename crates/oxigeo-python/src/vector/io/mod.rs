//! File I/O operations for vector data
//!
//! Provides GeoJSON and Shapefile reading/writing functions exposed to Python.

pub mod filtering;
pub mod geojson;

#[cfg(feature = "shapefile")]
pub mod shapefile;

// Re-export public functions
pub use geojson::{read_geojson, write_geojson};

#[cfg(feature = "shapefile")]
pub use shapefile::{read_shapefile, write_shapefile};

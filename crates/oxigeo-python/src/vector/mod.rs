//! Vector operations for Python bindings
//!
//! This module provides comprehensive Python-friendly interfaces to vector operations
//! including GeoJSON/Shapefile I/O, geometry operations, and spatial predicates.

mod analysis;
mod helpers;
mod io;
mod operations;
mod predicates;

// Re-export all public functions for use in lib.rs
pub use analysis::{
    clip_by_bbox, dissolve, distance, is_valid, make_valid, merge_polygons, transform,
};
pub use io::{read_geojson, write_geojson};
#[cfg(feature = "shapefile")]
pub use io::{read_shapefile, write_shapefile};
pub use operations::{
    area, buffer_geometry, centroid, convex_hull, difference, envelope, intersection, length,
    simplify, symmetric_difference, union,
};
pub use predicates::{contains, crosses, disjoint, intersects, overlaps, touches, within};

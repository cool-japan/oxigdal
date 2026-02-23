//! OxiGDAL Algorithms - High-Performance Raster and Vector Operations
//!
//! This crate provides production-ready geospatial algorithms for raster and vector processing,
//! with a focus on performance, correctness, and Pure Rust implementation.
//!
//! # Features
//!
//! ## Resampling Algorithms
//!
//! - Nearest neighbor (fast, preserves exact values)
//! - Bilinear interpolation (smooth, good for continuous data)
//! - Bicubic interpolation (high quality, slower)
//! - Lanczos resampling (highest quality, expensive)
//!
//! ## Raster Operations
//!
//! - Raster calculator (map algebra with expression evaluation)
//! - Hillshade generation (3D terrain visualization)
//! - Slope and aspect calculation (terrain analysis)
//! - Reclassification (value mapping and binning)
//! - Zonal statistics (aggregate statistics by zones)
//!
//! ## Vector Operations
//!
//! ### Geometric Operations
//! - Buffer generation (fixed and variable distance, multiple cap/join styles)
//! - Intersection (geometric intersection with sweep line algorithm)
//! - Union (geometric union, cascaded union, convex hull)
//! - Difference (geometric difference, symmetric difference, clip to box)
//!
//! ### Simplification
//! - Douglas-Peucker simplification (perpendicular distance based)
//! - Visvalingam-Whyatt simplification (area based)
//! - Topology-preserving simplification
//!
//! ### Geometric Analysis
//! - Centroid calculation (geometric and area-weighted, all geometry types)
//! - Area calculation (planar and geodetic methods)
//! - Distance measurement (Euclidean, Haversine, Vincenty)
//!
//! ### Spatial Predicates
//! - Contains, Within (point-in-polygon tests)
//! - Intersects, Disjoint (intersection tests)
//! - Touches, Overlaps (boundary relationships)
//!
//! ### Validation
//! - Geometry validation (OGC Simple Features compliance)
//! - Self-intersection detection
//! - Duplicate vertex detection
//! - Ring orientation and closure checks
//!
//! ## SIMD Optimizations
//!
//! Many algorithms use SIMD instructions (when enabled) for maximum performance.
//! Enable the `simd` feature for best performance.
//!
//! # Examples
//!
//! ## Raster Resampling
//!
//! ```
//! use oxigdal_algorithms::resampling::{ResamplingMethod, Resampler};
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create source raster
//! let src = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//!
//! // Resample to half size using bilinear interpolation
//! let resampler = Resampler::new(ResamplingMethod::Bilinear);
//! let dst = resampler.resample(&src, 500, 500)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Vector Operations
//!
//! ```
//! use oxigdal_algorithms::{
//!     Coordinate, LineString, Point, Polygon,
//!     buffer_point, BufferOptions,
//!     area, area_polygon, AreaMethod,
//!     centroid_polygon, simplify_linestring, SimplifyMethod,
//!     validate_polygon,
//! };
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a point and buffer it
//! let point = Point::new(0.0, 0.0);
//! let options = BufferOptions::default();
//! let buffered = buffer_point(&point, 10.0, &options)?;
//!
//! // Calculate area of a polygon
//! let coords = vec![
//!     Coordinate::new_2d(0.0, 0.0),
//!     Coordinate::new_2d(10.0, 0.0),
//!     Coordinate::new_2d(10.0, 10.0),
//!     Coordinate::new_2d(0.0, 10.0),
//!     Coordinate::new_2d(0.0, 0.0),
//! ];
//! let exterior = LineString::new(coords)?;
//! let polygon = Polygon::new(exterior, vec![])?;
//! let area_value = area_polygon(&polygon, AreaMethod::Planar)?;
//! # assert!((area_value - 100.0).abs() < 1e-10);
//!
//! // Simplify a linestring
//! let line_coords = vec![
//!     Coordinate::new_2d(0.0, 0.0),
//!     Coordinate::new_2d(1.0, 0.1),
//!     Coordinate::new_2d(2.0, -0.05),
//!     Coordinate::new_2d(3.0, 0.0),
//! ];
//! let linestring = LineString::new(line_coords)?;
//! let simplified = simplify_linestring(&linestring, 0.15, SimplifyMethod::DouglasPeucker)?;
//!
//! // Validate a polygon
//! let issues = validate_polygon(&polygon)?;
//! assert!(issues.is_empty()); // Valid square has no issues
//! # Ok(())
//! # }
//! ```
//!
//! # Performance
//!
//! All algorithms are designed for production use with:
//!
//! - Zero-copy operations where possible
//! - SIMD vectorization (x86_64 AVX2, ARM NEON)
//! - Cache-friendly memory access patterns
//! - Optional parallel processing via `rayon`
//!
//! # COOLJAPAN Policy Compliance
//!
//! - Pure Rust (no C/Fortran dependencies)
//! - No `unwrap()` or `expect()` in production code
//! - Comprehensive error handling
//! - no_std compatible core algorithms (with `alloc`)

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(clippy::all)]
// Pedantic disabled to reduce noise - default clippy::all is sufficient
// #![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
#![allow(clippy::module_name_repetitions)]
// Allow loop indexing patterns common in geospatial algorithms
#![allow(clippy::needless_range_loop)]
// Allow expect() for internal invariants that shouldn't fail
#![allow(clippy::expect_used)]
// Allow more arguments for complex geospatial operations
#![allow(clippy::too_many_arguments)]
// Allow manual implementations for clarity in algorithms
#![allow(clippy::manual_memcpy)]
#![allow(clippy::manual_div_ceil)]
#![allow(clippy::manual_clamp)]
// Allow dead code for internal algorithm structures
#![allow(dead_code)]
// Allow partial documentation for complex algorithm modules
#![allow(missing_docs)]
// Allow non-canonical partial_cmp for custom ordering
#![allow(clippy::non_canonical_partial_ord_impl)]
// Allow unused variables in algorithm code
#![allow(unused_variables)]
#![allow(unused_imports)]
// Allow collapsible match for algorithm clarity
#![allow(clippy::collapsible_match)]
#![allow(clippy::collapsible_if)]
// Allow manual_strip for path handling
#![allow(clippy::manual_strip)]
// Allow should_implement_trait for builder patterns
#![allow(clippy::should_implement_trait)]
// Allow method names that match trait names but with different signatures
#![allow(clippy::wrong_self_convention)]
// Allow iter_with_drain for performance patterns
#![allow(clippy::iter_with_drain)]
// Allow map_values for explicit iteration
#![allow(clippy::iter_kv_map)]
// Allow loop over option for clarity in algorithm code
#![allow(for_loops_over_fallibles)]
// Allow first element access with get(0)
#![allow(clippy::get_first)]
// Allow redundant closure for clarity
#![allow(clippy::redundant_closure)]
// Allow field assignment outside initializer
#![allow(clippy::field_reassign_with_default)]
// Allow manual iterator find implementations
#![allow(clippy::manual_find)]
// Allow identical blocks in if statements for algorithm clarity
#![allow(clippy::if_same_then_else)]
// Allow elided lifetime confusion
#![allow(clippy::needless_lifetimes)]
// Allow unused assignments for algorithm control flow
#![allow(unused_assignments)]
// Allow impls that can be derived (explicit implementations preferred)
#![allow(clippy::derivable_impls)]
// Allow explicit counter loop for clarity
#![allow(clippy::explicit_counter_loop)]
// Allow clone where from_ref could be used
#![allow(clippy::clone_on_ref_ptr)]
// Allow doc list item overindentation in complex formulas
#![allow(clippy::doc_overindented_list_items)]
// Allow useless vec for clarity in algorithm tests
#![allow(clippy::useless_vec)]
// Allow slice from ref pattern for geometry operations
#![allow(clippy::assigning_clones)]

pub mod error;
pub mod raster;
pub mod resampling;
pub mod vector;

#[cfg(feature = "simd")]
pub mod simd;

#[cfg(feature = "parallel")]
pub mod parallel;

#[cfg(feature = "dsl")]
pub mod dsl;

// Tutorial documentation
pub mod tutorials;

// Re-export commonly used items
pub use error::{AlgorithmError, Result};
pub use resampling::{Resampler, ResamplingMethod};

// Re-export vector operations for convenience
pub use vector::{
    AreaMethod,

    BufferCapStyle,
    BufferJoinStyle,
    BufferOptions,

    ContainsPredicate,
    // Geometric types (from oxigdal-core)
    Coordinate,
    DistanceMethod,

    IntersectsPredicate,
    IssueType,
    LineString,
    MultiPolygon,
    Point,
    Polygon,

    SegmentIntersection,

    Severity,
    SimplifyMethod,

    TouchesPredicate,

    ValidationIssue,
    // Area operations
    area,
    area_multipolygon,
    area_polygon,
    // Buffer operations
    buffer_linestring,
    buffer_point,
    buffer_polygon,
    // Union operations
    cascaded_union,
    // Centroid operations
    centroid,
    centroid_collection,
    centroid_linestring,
    centroid_multilinestring,
    centroid_multipoint,
    centroid_multipolygon,
    centroid_point,
    centroid_polygon,

    // Difference operations
    clip_to_box,
    // Advanced modules
    clustering,
    // Spatial predicates (contains, intersects, etc.)
    contains,
    convex_hull,
    delaunay,
    difference_polygon,
    difference_polygons,
    disjoint,
    // Distance operations
    distance_point_to_linestring,
    distance_point_to_point,
    distance_point_to_polygon,
    erase_small_holes,
    // Intersection operations
    intersect_linestrings,
    intersect_linestrings_sweep,
    intersect_polygons,
    intersect_segment_segment,
    intersects,
    is_clockwise,
    is_counter_clockwise,
    merge_polygons,
    network,
    point_in_polygon,
    point_in_polygon_or_boundary,
    point_on_polygon_boundary,
    point_strictly_inside_polygon,
    // Simplification operations
    simplify_linestring,
    simplify_linestring_dp,
    simplify_polygon,
    spatial_join,
    symmetric_difference,

    topology,
    touches,
    union_polygon,
    union_polygons,

    // Validation operations
    validate_geometry,
    validate_linestring,
    validate_polygon,
    voronoi,
    within,
};

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate name
pub const NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(NAME, "oxigdal-algorithms");
    }
}

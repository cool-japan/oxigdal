//! `oxigdal-index` — Pure-Rust spatial index (R-tree) for OxiGDAL vector data.
//!
//! # Overview
//!
//! This crate provides two complementary spatial indices:
//!
//! * [`RTree`] — an R-tree (linear-split variant) suitable for arbitrary data
//!   distributions.  Supports point / window queries and approximate k-nearest
//!   neighbours.
//! * [`GridIndex`] — a regular grid index that is faster for uniformly
//!   distributed data.
//!
//! Both indices operate on [`Bbox2D`] bounding boxes and store arbitrary
//! user-defined values.
//!
//! # Spatial queries
//!
//! [`SpatialQuery`] provides additional query helpers such as `within`,
//! `count_in`, and a spatial join.
//!
//! # Example
//!
//! ```rust
//! use oxigdal_index::{RTree, Bbox2D, SpatialQuery};
//!
//! let mut tree: RTree<&str> = RTree::new();
//! tree.insert(Bbox2D::new(0.0, 0.0, 2.0, 2.0).unwrap(), "polygon A");
//! tree.insert(Bbox2D::new(3.0, 3.0, 5.0, 5.0).unwrap(), "polygon B");
//!
//! let query = Bbox2D::new(1.0, 1.0, 4.0, 4.0).unwrap();
//! let hits = tree.search(&query);
//! assert_eq!(hits.len(), 2);
//!
//! let count = SpatialQuery::count_in(&tree, &query);
//! assert_eq!(count, 2);
//! ```

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

pub mod adaptive_grid;
pub mod bbox;
pub mod bbox3d;
pub mod bounding_circle;
pub mod clustering;
pub mod error;
pub mod geo_distance;
pub mod grid_index;
pub mod operations;
#[cfg(feature = "parallel")]
pub mod parallel_join;
pub mod polygon_boolean;
pub mod rtree;
pub mod rtree3d;
pub mod spatial_hash;
pub mod streaming_rtree;
pub mod sweep;
pub mod validation;
pub mod voronoi;

// Re-export the most important types at the crate root.
pub use adaptive_grid::AdaptiveGrid;
pub use bbox::Bbox2D;
pub use bbox3d::Bbox3D;
pub use bounding_circle::{
    BoundingCircle, smallest_enclosing_circle, smallest_enclosing_circle_from_bboxes,
};
pub use clustering::dbscan::{
    ClusterLabel, DbscanOptions, DbscanResult, NOISE, UNVISITED, dbscan_rtree, dbscan_with_rtree,
    range_query_eps,
};
pub use error::IndexError;
pub use geo_distance::{
    GeoNearestResult, GeoPoint, VincentyGeoResult, WGS84_A, WGS84_B, WGS84_INV_F,
    WGS84_MEAN_RADIUS_M, geo_bbox_extent_m, geo_nearest_k, geo_within_radius, haversine_m,
    haversine_m_with_radius, vincenty_inverse_wgs84,
};
pub use grid_index::GridIndex;
pub use operations::{
    area, buffer_bbox, centroid, convex_hull, distance, is_convex, perimeter, point_in_polygon,
    ring_bbox, simplify, simplify_visvalingam, simplify_visvalingam_to_count,
};
#[cfg(feature = "parallel")]
pub use parallel_join::{ParallelJoinOptions, spatial_join_parallel, spatial_join_with_options};
pub use polygon_boolean::{
    BooleanOp, BooleanResult, polygon_boolean, polygon_difference, polygon_intersection,
    polygon_symmetric_difference, polygon_union, polygons_intersect_bbox_test,
};
pub use rtree::hilbert::compute_hilbert_value;
pub use rtree::{HilbertRTree, RTree, SpatialQuery};
pub use rtree3d::RTree3D;
pub use spatial_hash::SpatialHashGrid;
pub use streaming_rtree::{StreamingInsertConfig, StreamingRTree, StreamingRTreeStats};
pub use sweep::{IntersectionPoint, Segment, find_all_intersections};
pub use validation::{
    Coord, MultiPolygon, Polygon, Ring, ValidationIssue, ValidationResult, validate_multipolygon,
    validate_no_self_intersection, validate_polygon, validate_ring_closure,
    validate_ring_orientation,
};
pub use voronoi::{
    VoronoiCell, VoronoiDiagram, VoronoiPoint, build_voronoi, cell_areas, circumcenter,
    find_cell_containing,
};

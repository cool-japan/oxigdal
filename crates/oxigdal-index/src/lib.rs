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

pub mod bbox;
pub mod error;
pub mod grid_index;
pub mod operations;
pub mod rtree;
pub mod validation;

// Re-export the most important types at the crate root.
pub use bbox::Bbox2D;
pub use error::IndexError;
pub use grid_index::GridIndex;
pub use operations::{
    area, buffer_bbox, centroid, convex_hull, distance, is_convex, perimeter, point_in_polygon,
    ring_bbox, simplify,
};
pub use rtree::{RTree, SpatialQuery};
pub use validation::{
    Coord, Polygon, Ring, ValidationIssue, ValidationResult, validate_no_self_intersection,
    validate_polygon, validate_ring_closure, validate_ring_orientation,
};

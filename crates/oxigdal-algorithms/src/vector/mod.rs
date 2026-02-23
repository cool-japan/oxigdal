//! Vector operations and geometric algorithms
//!
//! This module provides comprehensive geometric operations for vector data:
//!
//! ## Core Operations
//! - Buffer generation (fixed and variable distance)
//! - Intersection operations
//! - Union operations
//! - Difference operations
//!
//! ## Topology Operations
//! - Overlay analysis (intersection, union, difference, symmetric difference)
//! - Erase operations (cookie-cutter)
//! - Split operations (geometry splitting)
//!
//! ## Network Analysis
//! - Graph structures and building
//! - Shortest path algorithms (Dijkstra, A*, bidirectional)
//! - Service area calculation (isochrones)
//! - Advanced routing with constraints
//!
//! ## Spatial Clustering
//! - DBSCAN (density-based clustering)
//! - K-means clustering
//! - Hierarchical clustering
//!
//! ## Triangulation
//! - Delaunay triangulation
//! - Voronoi diagrams
//! - Constrained Delaunay
//!
//! ## Spatial Joins
//! - R-tree spatial index
//! - Nearest neighbor search
//! - K-nearest neighbors
//! - Range queries
//!
//! ## Simplification
//! - Douglas-Peucker algorithm
//! - Visvalingam-Whyatt algorithm
//! - Topology-preserving simplification
//!
//! ## Geometric Analysis
//! - Centroid calculation (geometric and area-weighted)
//! - Area calculation (planar and geodetic)
//! - Length measurement (planar and geodetic)
//! - Distance measurement (Euclidean, Haversine, Vincenty)
//! - Convex hull computation
//!
//! ## Spatial Predicates
//! - Contains, Within
//! - Intersects, Disjoint
//! - Touches, Overlaps, Crosses
//!
//! ## Validation & Repair
//! - Geometry validation
//! - Self-intersection detection
//! - Duplicate vertex detection
//! - Spike detection
//! - Geometry repair (fix orientation, remove duplicates, close rings)
//!
//! ## Coordinate Transformation
//! - CRS transformation (WGS84, Web Mercator, UTM, etc.)
//! - Geometry reprojection
//!
//! ## Object Pooling
//! - Thread-local object pools for Point, LineString, Polygon
//! - Pooled versions of buffer, union, difference, intersection operations
//! - Reduces allocations by 2-3x for batch operations
//! - Automatic return to pool via RAII guards
//!
//! ### Using Object Pooling
//!
//! ```
//! use oxigdal_algorithms::vector::{buffer_point_pooled, Point, BufferOptions};
//!
//! let point = Point::new(0.0, 0.0);
//! let options = BufferOptions::default();
//!
//! // Get a pooled polygon - automatically returned to pool when dropped
//! let buffered = buffer_point_pooled(&point, 10.0, &options)?;
//! // Use buffered geometry...
//! // Polygon returned to pool when `buffered` goes out of scope
//! # Ok::<(), oxigdal_algorithms::error::AlgorithmError>(())
//! ```
//!
//! All operations use geometry types from `oxigdal-core::vector`.

mod area;
mod buffer;
mod centroid;
pub mod clustering;
mod contains;
pub mod delaunay;
mod difference;
mod distance;
mod douglas_peucker;
mod envelope;
mod intersection;
mod length;
pub mod network;
pub mod pool;
mod repair;
mod simplify;
pub mod spatial_join;
pub mod topology;
mod transform;
mod union_ops;
mod valid;
pub mod voronoi;

// Re-export from core
pub use oxigdal_core::vector::{Coordinate, LineString, MultiPolygon, Point, Polygon};

// Re-export area operations
pub use area::{
    AreaMethod, area, area_multipolygon, area_polygon, is_clockwise, is_counter_clockwise,
};

// Re-export buffer operations
pub use buffer::{
    BufferCapStyle, BufferJoinStyle, BufferOptions, buffer_linestring, buffer_linestring_pooled,
    buffer_point, buffer_point_pooled, buffer_polygon, buffer_polygon_pooled,
};

// Re-export centroid operations
pub use centroid::{
    centroid, centroid_collection, centroid_linestring, centroid_multilinestring,
    centroid_multipoint, centroid_multipolygon, centroid_point, centroid_polygon,
};

// Re-export spatial predicates
pub use contains::{
    ContainsPredicate, CrossesPredicate, IntersectsPredicate, OverlapsPredicate, TouchesPredicate,
    contains, crosses, disjoint, intersects, overlaps, point_in_polygon_or_boundary,
    point_on_polygon_boundary, point_strictly_inside_polygon, touches, within,
};

// Re-export difference operations
pub use difference::{
    clip_to_box, difference_polygon, difference_polygon_pooled, difference_polygons,
    difference_polygons_pooled, erase_small_holes, symmetric_difference,
    symmetric_difference_pooled,
};

// Re-export distance operations
pub use distance::{
    DistanceMethod, distance_point_to_linestring, distance_point_to_point,
    distance_point_to_polygon,
};

// Re-export Douglas-Peucker
pub use douglas_peucker::simplify_linestring as simplify_linestring_dp;

// Re-export envelope operations
pub use envelope::{
    envelope, envelope_collection, envelope_contains_point, envelope_intersection,
    envelope_linestring, envelope_multilinestring, envelope_multipoint, envelope_multipolygon,
    envelope_point, envelope_polygon, envelope_union, envelope_with_buffer, envelopes_intersect,
};

// Re-export intersection operations
pub use intersection::{
    SegmentIntersection, intersect_linestrings, intersect_linestrings_sweep, intersect_polygons,
    intersect_polygons_pooled, intersect_segment_segment, point_in_polygon,
};

// Re-export length operations
pub use length::{
    LengthMethod, length, length_linestring, length_linestring_3d, length_multilinestring,
};

// Re-export simplification operations
pub use simplify::{SimplifyMethod, simplify_linestring, simplify_polygon};

// Re-export union operations
pub use union_ops::{
    cascaded_union, cascaded_union_pooled, convex_hull, convex_hull_pooled, merge_polygons,
    union_polygon, union_polygon_pooled, union_polygons, union_polygons_pooled,
};

// Re-export pool types and utilities
pub use pool::{
    Pool, PoolGuard, PoolStats, clear_all_pools, get_pool_stats, get_pooled_coordinate_vec,
    get_pooled_linestring, get_pooled_point, get_pooled_polygon,
};

// Re-export validation
pub use valid::{
    IssueType, Severity, ValidationIssue, validate_geometry, validate_linestring, validate_polygon,
};

// Re-export repair operations
pub use repair::{
    RepairOptions, close_ring, fix_self_intersection, remove_collinear_vertices,
    remove_duplicate_vertices, remove_spikes, repair_linestring, repair_linestring_with_options,
    repair_polygon, repair_polygon_with_options, reverse_ring,
};

// Re-export coordinate transformations
pub use transform::{
    CommonCrs, CrsTransformer, transform_geometry, transform_linestring, transform_point,
    transform_polygon,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_distance() {
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(3.0, 4.0);
        let dx = p1.x - p2.x;
        let dy = p1.y - p2.y;
        let dist = (dx * dx + dy * dy).sqrt();
        assert!((dist - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_linestring_construction() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(3.0, 0.0),
            Coordinate::new_2d(3.0, 4.0),
        ];
        let result = LineString::new(coords);
        assert!(result.is_ok());
        if let Ok(line) = result {
            assert_eq!(line.len(), 3);
        }
    }

    #[test]
    fn test_polygon_construction() {
        // Square with side length 10
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(0.0, 10.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(coords);
        assert!(exterior.is_ok());
        if let Ok(ext) = exterior {
            let poly = Polygon::new(ext, vec![]);
            assert!(poly.is_ok());
        }
    }
}

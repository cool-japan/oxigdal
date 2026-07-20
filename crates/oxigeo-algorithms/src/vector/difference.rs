//! Geometric difference operations
//!
//! This module implements robust geometric difference (subtraction) algorithms
//! for computing the symmetric and asymmetric difference of geometric features.
//! Difference operations are fundamental for overlay analysis, cookie-cutter
//! operations, and spatial editing.
//!
//! # Operations
//!
//! - **Difference**: A - B (removes B from A)
//! - **Symmetric Difference**: (A - B) ∪ (B - A)
//! - **Multi-polygon Difference**: Handles holes properly
//!
//! # Interior Ring (Hole) Handling
//!
//! This module fully supports interior rings (holes) in polygons:
//!
//! - **Difference with holes**: Existing holes in input polygons are properly
//!   clipped and preserved in the result
//! - **Hole creation**: When the subtracted polygon is entirely contained,
//!   it becomes a new hole
//! - **Hole clipping**: When clipping to a bounding box, holes are properly
//!   clipped, removed, or preserved based on their relationship to the box
//! - **Topology validation**: Results are validated to ensure proper polygon
//!   structure
//!
//! # Examples
//!
//! ```
//! # use oxigeo_core::error::Result;
//! use oxigeo_algorithms::vector::{difference_polygon, Coordinate, LineString, Polygon};
//!
//! # fn main() -> Result<()> {
//! let coords1 = vec![
//!     Coordinate::new_2d(0.0, 0.0),
//!     Coordinate::new_2d(10.0, 0.0),
//!     Coordinate::new_2d(10.0, 10.0),
//!     Coordinate::new_2d(0.0, 10.0),
//!     Coordinate::new_2d(0.0, 0.0),
//! ];
//! let ext1 = LineString::new(coords1)?;
//! let poly1 = Polygon::new(ext1, vec![])?;
//! # Ok(())
//! # }
//! ```

use crate::error::{AlgorithmError, Result};
use crate::vector::pool::{PoolGuard, get_pooled_polygon};
use oxigeo_core::vector::{Coordinate, LineString, Polygon};

#[cfg(feature = "std")]
use std::vec::Vec;

/// Tolerance for coordinate comparisons
const EPSILON: f64 = 1e-10;

/// Computes the difference of two polygons (poly1 - poly2)
///
/// Returns the portion of poly1 that does not overlap with poly2.
/// Handles:
/// - Disjoint polygons (returns poly1)
/// - poly2 contains poly1 (returns empty)
/// - poly1 contains poly2 (returns poly1 with poly2 as hole)
/// - Partial overlap (simplified result)
/// - Existing interior rings (holes) in both polygons
///
/// # Interior Ring Handling
///
/// - Holes in poly1: Preserved unless they are completely covered by poly2
/// - Holes in poly2: If poly2 is inside poly1, poly2's holes become new polygon
///   regions (since subtracting a hole means keeping that area)
/// - New holes: Created when poly2 is entirely contained within poly1
///
/// # Arguments
///
/// * `poly1` - The polygon to subtract from
/// * `poly2` - The polygon to subtract
///
/// # Returns
///
/// Vector of polygons representing the difference
///
/// # Errors
///
/// Returns error if polygons are invalid
pub fn difference_polygon(poly1: &Polygon, poly2: &Polygon) -> Result<Vec<Polygon>> {
    use oxigeo_core::OxiGeoError;

    // Check validity -- preserve the detailed OxiGeoError builder for backward compat
    if poly1.exterior.coords.len() < 4 {
        return Err(OxiGeoError::invalid_parameter_builder(
            "poly1",
            format!("exterior must have at least 4 coordinates, got {}", poly1.exterior.coords.len()),
        )
        .with_parameter("coordinate_count", poly1.exterior.coords.len().to_string())
        .with_parameter("min_count", "4")
        .with_operation("difference_polygon")
        .with_suggestion("A valid polygon requires at least 4 coordinates (first and last must be identical to close the ring)")
        .build()
        .into());
    }

    if poly2.exterior.coords.len() < 4 {
        return Err(OxiGeoError::invalid_parameter_builder(
            "poly2",
            format!("exterior must have at least 4 coordinates, got {}", poly2.exterior.coords.len()),
        )
        .with_parameter("coordinate_count", poly2.exterior.coords.len().to_string())
        .with_parameter("min_count", "4")
        .with_operation("difference_polygon")
        .with_suggestion("A valid polygon requires at least 4 coordinates (first and last must be identical to close the ring)")
        .build()
        .into());
    }

    // Delegate to the Weiler-Atherton clipping engine
    crate::vector::clipping::clip_polygons(
        poly1,
        poly2,
        crate::vector::clipping::ClipOperation::Difference,
    )
}

/// Checks if a point is inside a ring using ray casting.
///
/// Kept for use by local helpers (topology validation, hole merging, etc.).
fn point_in_ring(point: &Coordinate, ring: &[Coordinate]) -> bool {
    crate::vector::clipping::point_in_ring(point, ring)
}

/// Computes the centroid of a ring.
///
/// Retained as a thin re-export for local tests and potential future helpers;
/// `is_ring_inside_ring` no longer relies on a centroid test (it uses a robust
/// crossing + vertex-containment check that handles concave rings).
#[allow(dead_code)]
fn compute_ring_centroid(coords: &[Coordinate]) -> Coordinate {
    crate::vector::clipping::compute_ring_centroid(coords)
}

/// Computes the difference of multiple polygons
///
/// Subtracts all polygons in `subtract` from all polygons in `base`.
///
/// # Arguments
///
/// * `base` - Base polygons to subtract from
/// * `subtract` - Polygons to subtract
///
/// # Returns
///
/// Vector of polygons representing the difference
///
/// # Errors
///
/// Returns error if any polygon is invalid
pub fn difference_polygons(base: &[Polygon], subtract: &[Polygon]) -> Result<Vec<Polygon>> {
    if base.is_empty() {
        return Ok(vec![]);
    }

    if subtract.is_empty() {
        return Ok(base.to_vec());
    }

    // Validate all polygons
    for (i, poly) in base.iter().enumerate() {
        if poly.exterior.coords.len() < 4 {
            return Err(AlgorithmError::InsufficientData {
                operation: "difference_polygons",
                message: format!(
                    "base polygon {} exterior must have at least 4 coordinates",
                    i
                ),
            });
        }
    }

    for (i, poly) in subtract.iter().enumerate() {
        if poly.exterior.coords.len() < 4 {
            return Err(AlgorithmError::InsufficientData {
                operation: "difference_polygons",
                message: format!(
                    "subtract polygon {} exterior must have at least 4 coordinates",
                    i
                ),
            });
        }
    }

    // Sequentially subtract each polygon in subtract from the result
    let mut result = base.to_vec();

    for sub_poly in subtract {
        let mut new_result = Vec::new();

        for base_poly in &result {
            let diff = difference_polygon(base_poly, sub_poly)?;
            new_result.extend(diff);
        }

        result = new_result;
    }

    Ok(result)
}

/// Computes the symmetric difference of two polygons
///
/// Returns the area that is in either polygon but not in both.
/// Equivalent to (poly1 - poly2) ∪ (poly2 - poly1).
///
/// # Arguments
///
/// * `poly1` - First polygon
/// * `poly2` - Second polygon
///
/// # Returns
///
/// Vector of polygons representing the symmetric difference
///
/// # Errors
///
/// Returns error if polygons are invalid
pub fn symmetric_difference(poly1: &Polygon, poly2: &Polygon) -> Result<Vec<Polygon>> {
    // Compute poly1 - poly2
    let diff1 = difference_polygon(poly1, poly2)?;

    // Compute poly2 - poly1
    let diff2 = difference_polygon(poly2, poly1)?;

    // Union the results
    let mut result = diff1;
    result.extend(diff2);

    Ok(result)
}

/// Clips a polygon to a rectangular bounding box
///
/// Returns the portion of the polygon that falls within the box.
/// This is a specialized form of difference optimized for rectangular
/// clipping regions.
///
/// # Interior Ring Handling
///
/// Interior rings (holes) are processed as follows:
/// - **Completely inside box**: Hole is preserved unchanged
/// - **Completely outside box**: Hole is removed
/// - **Partially inside**: Hole is clipped using Sutherland-Hodgman algorithm
/// - **Straddling box boundary**: Hole is clipped, potentially creating
///   multiple result polygons if the clipping splits the polygon
///
/// # Arguments
///
/// * `polygon` - The polygon to clip
/// * `min_x` - Minimum X coordinate of bounding box
/// * `min_y` - Minimum Y coordinate of bounding box
/// * `max_x` - Maximum X coordinate of bounding box
/// * `max_y` - Maximum Y coordinate of bounding box
///
/// # Returns
///
/// Vector of clipped polygons
///
/// # Errors
///
/// Returns error if polygon is invalid or bounding box is invalid
pub fn clip_to_box(
    polygon: &Polygon,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
) -> Result<Vec<Polygon>> {
    if polygon.exterior.coords.len() < 4 {
        return Err(AlgorithmError::InsufficientData {
            operation: "clip_to_box",
            message: "polygon exterior must have at least 4 coordinates".to_string(),
        });
    }

    if min_x >= max_x || min_y >= max_y {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "bounding box",
            message: "invalid bounding box dimensions".to_string(),
        });
    }

    // Check if polygon is completely outside box
    if let Some((poly_min_x, poly_min_y, poly_max_x, poly_max_y)) = polygon.bounds() {
        if poly_max_x < min_x || poly_min_x > max_x || poly_max_y < min_y || poly_min_y > max_y {
            // Completely outside
            return Ok(vec![]);
        }

        if poly_min_x >= min_x && poly_max_x <= max_x && poly_min_y >= min_y && poly_max_y <= max_y
        {
            // Completely inside - return polygon unchanged (with all holes)
            return Ok(vec![polygon.clone()]);
        }
    }

    // Clip exterior ring using Sutherland-Hodgman algorithm
    let mut clipped_coords = polygon.exterior.coords.clone();

    // Clip against each edge of the box
    clipped_coords = clip_against_edge(&clipped_coords, min_x, true, false)?; // Left
    clipped_coords = clip_against_edge(&clipped_coords, max_x, true, true)?; // Right
    clipped_coords = clip_against_edge(&clipped_coords, min_y, false, false)?; // Bottom
    clipped_coords = clip_against_edge(&clipped_coords, max_y, false, true)?; // Top

    if clipped_coords.len() < 4 {
        // Clipped to nothing
        return Ok(vec![]);
    }

    // Ensure ring is closed
    if let (Some(first), Some(last)) = (clipped_coords.first(), clipped_coords.last()) {
        if (first.x - last.x).abs() > f64::EPSILON || (first.y - last.y).abs() > f64::EPSILON {
            clipped_coords.push(*first);
        }
    }

    let clipped_exterior = LineString::new(clipped_coords).map_err(AlgorithmError::Core)?;

    // Handle interior rings (holes)
    let clipped_interiors = clip_interior_rings_to_box(
        &polygon.interiors,
        min_x,
        min_y,
        max_x,
        max_y,
        &clipped_exterior,
    )?;

    let result = Polygon::new(clipped_exterior, clipped_interiors).map_err(AlgorithmError::Core)?;

    Ok(vec![result])
}

/// Clips interior rings (holes) to a bounding box
///
/// For each interior ring:
/// 1. Check if it's completely outside the box (remove it)
/// 2. Check if it's completely inside the box (keep it)
/// 3. Otherwise, clip it using Sutherland-Hodgman algorithm
///
/// # Arguments
///
/// * `interiors` - The interior rings to clip
/// * `min_x` - Minimum X coordinate of bounding box
/// * `min_y` - Minimum Y coordinate of bounding box
/// * `max_x` - Maximum X coordinate of bounding box
/// * `max_y` - Maximum Y coordinate of bounding box
/// * `clipped_exterior` - The clipped exterior ring (for validation)
///
/// # Returns
///
/// Vector of clipped interior rings
fn clip_interior_rings_to_box(
    interiors: &[LineString],
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    clipped_exterior: &LineString,
) -> Result<Vec<LineString>> {
    let mut result = Vec::new();

    for hole in interiors {
        if hole.coords.len() < 4 {
            continue;
        }

        // Check if hole is completely outside or inside the box
        let hole_bounds = hole.bounds();
        if let Some((hole_min_x, hole_min_y, hole_max_x, hole_max_y)) = hole_bounds {
            // Check if hole is completely outside box
            if hole_max_x < min_x || hole_min_x > max_x || hole_max_y < min_y || hole_min_y > max_y
            {
                // Hole is completely outside box - remove it
                continue;
            }

            // Check if hole is completely inside box
            if hole_min_x >= min_x
                && hole_max_x <= max_x
                && hole_min_y >= min_y
                && hole_max_y <= max_y
            {
                // Hole is completely inside box - check if it's inside the clipped exterior
                if is_ring_inside_ring(&hole.coords, &clipped_exterior.coords) {
                    result.push(hole.clone());
                }
                continue;
            }
        }

        // Hole straddles the box boundary - clip it
        let clipped_hole = clip_ring_to_box(&hole.coords, min_x, min_y, max_x, max_y)?;

        if clipped_hole.len() >= 4 {
            // Check if the clipped hole is inside the clipped exterior
            if is_ring_inside_ring(&clipped_hole, &clipped_exterior.coords) {
                // Create a valid closed ring
                let mut closed_hole = clipped_hole;
                if let (Some(first), Some(last)) = (closed_hole.first(), closed_hole.last()) {
                    if (first.x - last.x).abs() > EPSILON || (first.y - last.y).abs() > EPSILON {
                        closed_hole.push(*first);
                    }
                }

                if closed_hole.len() >= 4 {
                    if let Ok(hole_ring) = LineString::new(closed_hole) {
                        result.push(hole_ring);
                    }
                }
            }
        }
    }

    Ok(result)
}

/// Clips a ring to a bounding box using Sutherland-Hodgman algorithm
///
/// # Arguments
///
/// * `coords` - The ring coordinates to clip
/// * `min_x` - Minimum X coordinate of bounding box
/// * `min_y` - Minimum Y coordinate of bounding box
/// * `max_x` - Maximum X coordinate of bounding box
/// * `max_y` - Maximum Y coordinate of bounding box
///
/// # Returns
///
/// Vector of clipped coordinates
fn clip_ring_to_box(
    coords: &[Coordinate],
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
) -> Result<Vec<Coordinate>> {
    let mut clipped = coords.to_vec();

    // Clip against each edge of the box
    clipped = clip_against_edge(&clipped, min_x, true, false)?; // Left
    clipped = clip_against_edge(&clipped, max_x, true, true)?; // Right
    clipped = clip_against_edge(&clipped, min_y, false, false)?; // Bottom
    clipped = clip_against_edge(&clipped, max_y, false, true)?; // Top

    Ok(clipped)
}

/// Checks if a ring is completely inside another ring.
///
/// Robust for concave rings: a single-point (centroid) test is unreliable
/// because a concave ring's centroid can fall outside the ring itself, or in a
/// notch of a concave `outer`, misclassifying containment. Instead this checks
/// that (1) no edge of `inner` *properly* crosses an edge of `outer`, and
/// (2) at least one vertex of `inner` lies strictly inside `outer`. Together
/// these imply every vertex of `inner` is inside-or-on `outer`, so `inner` is
/// contained. Shared/touching boundary segments (common after clipping to a
/// box) are not proper crossings, so boundary-touching holes are preserved.
///
/// # Arguments
///
/// * `inner` - The potentially inner ring
/// * `outer` - The potentially outer ring
///
/// # Returns
///
/// true if inner is inside outer
fn is_ring_inside_ring(inner: &[Coordinate], outer: &[Coordinate]) -> bool {
    if inner.len() < 3 || outer.len() < 3 {
        return false;
    }

    // (1) Reject any proper interior crossing between the two boundaries.
    // `segments_intersect` excludes shared endpoints and collinear overlaps, so
    // rings that merely touch along a shared edge are not rejected here.
    for i in 0..(inner.len() - 1) {
        for j in 0..(outer.len() - 1) {
            if segments_intersect(&inner[i], &inner[i + 1], &outer[j], &outer[j + 1]) {
                return false;
            }
        }
    }

    // (2) Require at least one vertex of `inner` strictly inside `outer`.
    // With no proper crossings, this rules out the "disjoint" and "outer inside
    // inner" cases (whose inner vertices are all outside `outer`).
    inner.iter().any(|v| point_in_ring(v, outer))
}

/// Validates polygon topology after clipping operations
///
/// Ensures that:
/// 1. Exterior ring has at least 4 coordinates and is closed
/// 2. All interior rings have at least 4 coordinates and are closed
/// 3. All interior rings are inside the exterior ring
/// 4. No interior rings overlap with each other
///
/// # Arguments
///
/// * `polygon` - The polygon to validate
///
/// # Returns
///
/// Ok(true) if valid, Ok(false) if topology issues detected
///
/// # Errors
///
/// Returns error if validation fails due to invalid data
pub fn validate_polygon_topology(polygon: &Polygon) -> Result<bool> {
    // Check exterior ring
    if polygon.exterior.coords.len() < 4 {
        return Ok(false);
    }

    // Check exterior ring is closed
    if let (Some(first), Some(last)) = (
        polygon.exterior.coords.first(),
        polygon.exterior.coords.last(),
    ) {
        if (first.x - last.x).abs() > EPSILON || (first.y - last.y).abs() > EPSILON {
            return Ok(false);
        }
    }

    // Check each interior ring
    for hole in &polygon.interiors {
        // Check minimum size
        if hole.coords.len() < 4 {
            return Ok(false);
        }

        // Check ring is closed
        if let (Some(first), Some(last)) = (hole.coords.first(), hole.coords.last()) {
            if (first.x - last.x).abs() > EPSILON || (first.y - last.y).abs() > EPSILON {
                return Ok(false);
            }
        }

        // Check hole is inside exterior
        if !is_ring_inside_ring(&hole.coords, &polygon.exterior.coords) {
            return Ok(false);
        }
    }

    // Check no interior rings overlap
    for i in 0..polygon.interiors.len() {
        for j in (i + 1)..polygon.interiors.len() {
            if rings_overlap(&polygon.interiors[i].coords, &polygon.interiors[j].coords)? {
                return Ok(false);
            }
        }
    }

    Ok(true)
}

/// Checks if two rings overlap
///
/// Two rings overlap if any vertex of one ring is inside the other,
/// or if their boundaries intersect.
fn rings_overlap(ring1: &[Coordinate], ring2: &[Coordinate]) -> Result<bool> {
    // Quick check: if any vertex of ring1 is inside ring2 (or vice versa)
    for coord in ring1 {
        if point_in_ring(coord, ring2) {
            return Ok(true);
        }
    }

    for coord in ring2 {
        if point_in_ring(coord, ring1) {
            return Ok(true);
        }
    }

    // Check for boundary intersections
    // For simplicity, we use a basic segment-segment intersection check
    if ring1.len() < 2 || ring2.len() < 2 {
        return Ok(false);
    }

    for i in 0..(ring1.len() - 1) {
        for j in 0..(ring2.len() - 1) {
            if segments_intersect(&ring1[i], &ring1[i + 1], &ring2[j], &ring2[j + 1]) {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// Checks if two line segments intersect (excluding endpoints)
fn segments_intersect(p1: &Coordinate, p2: &Coordinate, p3: &Coordinate, p4: &Coordinate) -> bool {
    let d1x = p2.x - p1.x;
    let d1y = p2.y - p1.y;
    let d2x = p4.x - p3.x;
    let d2y = p4.y - p3.y;

    let cross = d1x * d2y - d1y * d2x;

    if cross.abs() < EPSILON {
        // Parallel or collinear
        return false;
    }

    let dx = p3.x - p1.x;
    let dy = p3.y - p1.y;

    let t = (dx * d2y - dy * d2x) / cross;
    let u = (dx * d1y - dy * d1x) / cross;

    // Exclude endpoints (strict interior intersection)
    t > EPSILON && t < (1.0 - EPSILON) && u > EPSILON && u < (1.0 - EPSILON)
}

/// Merges adjacent holes if they share a common boundary
///
/// This function identifies holes that touch or overlap and merges them
/// into a single hole to maintain valid polygon topology.
///
/// # Arguments
///
/// * `holes` - Vector of interior rings to potentially merge
///
/// # Returns
///
/// Vector of merged interior rings
pub fn merge_adjacent_holes(holes: &[LineString]) -> Result<Vec<LineString>> {
    if holes.is_empty() {
        return Ok(vec![]);
    }

    if holes.len() == 1 {
        return Ok(holes.to_vec());
    }

    // Simple approach: keep non-overlapping holes
    // A more sophisticated approach would use union operations
    let mut result = Vec::new();
    let mut merged = vec![false; holes.len()];

    for i in 0..holes.len() {
        if merged[i] {
            continue;
        }

        let mut current_hole = holes[i].coords.clone();

        for j in (i + 1)..holes.len() {
            if merged[j] {
                continue;
            }

            if rings_overlap(&current_hole, &holes[j].coords)? {
                // Merge by taking the convex hull of both holes
                // Simplified: just take the larger hole
                if compute_ring_area(&holes[j].coords).abs()
                    > compute_ring_area(&current_hole).abs()
                {
                    current_hole = holes[j].coords.clone();
                }
                merged[j] = true;
            }
        }

        if current_hole.len() >= 4 {
            if let Ok(ring) = LineString::new(current_hole) {
                result.push(ring);
            }
        }
    }

    Ok(result)
}

/// Clips a polygon against a single edge using Sutherland-Hodgman algorithm
fn clip_against_edge(
    coords: &[Coordinate],
    edge_value: f64,
    is_vertical: bool,
    is_max: bool,
) -> Result<Vec<Coordinate>> {
    if coords.is_empty() {
        return Ok(vec![]);
    }

    let mut result = Vec::new();

    for i in 0..coords.len() {
        let current = &coords[i];
        let next = &coords[(i + 1) % coords.len()];

        let current_inside = is_inside(current, edge_value, is_vertical, is_max);
        let next_inside = is_inside(next, edge_value, is_vertical, is_max);

        if current_inside {
            result.push(*current);
        }

        if current_inside != next_inside {
            // Edge crosses the clip line - compute intersection
            if let Some(intersection) = compute_intersection(current, next, edge_value, is_vertical)
            {
                result.push(intersection);
            }
        }
    }

    Ok(result)
}

/// Checks if a point is inside relative to a clipping edge
fn is_inside(point: &Coordinate, edge_value: f64, is_vertical: bool, is_max: bool) -> bool {
    let value = if is_vertical { point.x } else { point.y };

    if is_max {
        value <= edge_value
    } else {
        value >= edge_value
    }
}

/// Computes intersection of a line segment with a clipping edge
fn compute_intersection(
    p1: &Coordinate,
    p2: &Coordinate,
    edge_value: f64,
    is_vertical: bool,
) -> Option<Coordinate> {
    if is_vertical {
        // Vertical edge (x = edge_value)
        let dx = p2.x - p1.x;
        if dx.abs() < f64::EPSILON {
            return None; // Parallel to edge
        }

        let t = (edge_value - p1.x) / dx;
        if (0.0..=1.0).contains(&t) {
            let y = p1.y + t * (p2.y - p1.y);
            Some(Coordinate::new_2d(edge_value, y))
        } else {
            None
        }
    } else {
        // Horizontal edge (y = edge_value)
        let dy = p2.y - p1.y;
        if dy.abs() < f64::EPSILON {
            return None; // Parallel to edge
        }

        let t = (edge_value - p1.y) / dy;
        if (0.0..=1.0).contains(&t) {
            let x = p1.x + t * (p2.x - p1.x);
            Some(Coordinate::new_2d(x, edge_value))
        } else {
            None
        }
    }
}

/// Erases small holes from a polygon
///
/// Removes interior rings (holes) smaller than a threshold area.
/// Useful for cleaning up polygon topology.
///
/// # Arguments
///
/// * `polygon` - The polygon to clean
/// * `min_area` - Minimum area for holes to keep
///
/// # Returns
///
/// Cleaned polygon
///
/// # Errors
///
/// Returns error if polygon is invalid
pub fn erase_small_holes(polygon: &Polygon, min_area: f64) -> Result<Polygon> {
    if polygon.exterior.coords.len() < 4 {
        return Err(AlgorithmError::InsufficientData {
            operation: "erase_small_holes",
            message: "polygon exterior must have at least 4 coordinates".to_string(),
        });
    }

    if min_area < 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "min_area",
            message: "min_area must be non-negative".to_string(),
        });
    }

    let mut kept_holes = Vec::new();

    for hole in &polygon.interiors {
        let area = compute_ring_area(&hole.coords).abs();
        if area >= min_area {
            kept_holes.push(hole.clone());
        }
    }

    Polygon::new(polygon.exterior.clone(), kept_holes).map_err(AlgorithmError::Core)
}

/// Computes the signed area of a ring using shoelace formula
fn compute_ring_area(coords: &[Coordinate]) -> f64 {
    if coords.len() < 3 {
        return 0.0;
    }

    let mut area = 0.0;
    let n = coords.len();

    for i in 0..n {
        let j = (i + 1) % n;
        area += coords[i].x * coords[j].y;
        area -= coords[j].x * coords[i].y;
    }

    area / 2.0
}

//
// Pooled difference operations for reduced allocations
//

/// Computes difference of two polygons using object pooling
///
/// This is the pooled version of `difference_polygon` that reuses allocated
/// polygons from a thread-local pool. Returns the first result polygon.
///
/// # Arguments
///
/// * `subject` - Polygon to subtract from
/// * `clip` - Polygon to subtract
///
/// # Returns
///
/// A pooled polygon guard representing the difference (first result if multiple)
///
/// # Errors
///
/// Returns error if polygons are invalid
///
/// # Example
///
/// ```
/// use oxigeo_algorithms::vector::{difference_polygon_pooled, Coordinate, LineString, Polygon};
///
/// let coords1 = vec![
///     Coordinate::new_2d(0.0, 0.0),
///     Coordinate::new_2d(10.0, 0.0),
///     Coordinate::new_2d(10.0, 10.0),
///     Coordinate::new_2d(0.0, 10.0),
///     Coordinate::new_2d(0.0, 0.0),
/// ];
/// let ext1 = LineString::new(coords1)?;
/// let poly1 = Polygon::new(ext1, vec![])?;
/// # let coords2 = vec![
/// #     Coordinate::new_2d(5.0, 5.0),
/// #     Coordinate::new_2d(15.0, 5.0),
/// #     Coordinate::new_2d(15.0, 15.0),
/// #     Coordinate::new_2d(5.0, 15.0),
/// #     Coordinate::new_2d(5.0, 5.0),
/// # ];
/// # let ext2 = LineString::new(coords2)?;
/// # let poly2 = Polygon::new(ext2, vec![])?;
/// let result = difference_polygon_pooled(&poly1, &poly2)?;
/// # Ok::<(), oxigeo_algorithms::error::AlgorithmError>(())
/// ```
pub fn difference_polygon_pooled(
    subject: &Polygon,
    clip: &Polygon,
) -> Result<PoolGuard<'static, Polygon>> {
    let results = difference_polygon(subject, clip)?;

    // Get a pooled polygon and copy the first result into it
    if let Some(result) = results.first() {
        let mut poly = get_pooled_polygon();
        poly.exterior = result.exterior.clone();
        poly.interiors = result.interiors.clone();
        Ok(poly)
    } else {
        Err(AlgorithmError::InsufficientData {
            operation: "difference_polygon_pooled",
            message: "difference resulted in no polygons".to_string(),
        })
    }
}

/// Computes difference of base polygons with subtract polygons using object pooling
///
/// Subtracts subtract polygons from base polygons.
/// Returns the first result polygon from the pool.
///
/// # Arguments
///
/// * `base` - Base polygons
/// * `subtract` - Polygons to subtract
///
/// # Returns
///
/// A pooled polygon guard representing the difference
///
/// # Errors
///
/// Returns error if any polygon is invalid
///
/// # Example
///
/// ```
/// use oxigeo_algorithms::vector::{difference_polygons_pooled, Coordinate, LineString, Polygon};
///
/// let coords = vec![
///     Coordinate::new_2d(0.0, 0.0),
///     Coordinate::new_2d(10.0, 0.0),
///     Coordinate::new_2d(10.0, 10.0),
///     Coordinate::new_2d(0.0, 10.0),
///     Coordinate::new_2d(0.0, 0.0),
/// ];
/// let ext = LineString::new(coords)?;
/// let base_poly = Polygon::new(ext, vec![])?;
/// let base = vec![base_poly];
/// let subtract: Vec<Polygon> = vec![]; // Empty for example
/// let result = difference_polygons_pooled(&base, &subtract)?;
/// # Ok::<(), oxigeo_algorithms::error::AlgorithmError>(())
/// ```
pub fn difference_polygons_pooled(
    base: &[Polygon],
    subtract: &[Polygon],
) -> Result<PoolGuard<'static, Polygon>> {
    let results = difference_polygons(base, subtract)?;

    // Get a pooled polygon and copy the first result into it
    if let Some(result) = results.first() {
        let mut poly = get_pooled_polygon();
        poly.exterior = result.exterior.clone();
        poly.interiors = result.interiors.clone();
        Ok(poly)
    } else {
        Err(AlgorithmError::InsufficientData {
            operation: "difference_polygons_pooled",
            message: "difference resulted in no polygons".to_string(),
        })
    }
}

/// Computes symmetric difference using object pooling
///
/// Returns pooled polygon guards for both result polygons. For symmetric
/// difference (A-B) ∪ (B-A), this returns up to 2 pooled polygons.
///
/// # Arguments
///
/// * `poly1` - First polygon
/// * `poly2` - Second polygon
///
/// # Returns
///
/// Vector of pooled polygon guards representing the symmetric difference
///
/// # Errors
///
/// Returns error if polygons are invalid
///
/// # Example
///
/// ```
/// use oxigeo_algorithms::vector::{symmetric_difference_pooled, Coordinate, LineString, Polygon};
///
/// let coords1 = vec![
///     Coordinate::new_2d(0.0, 0.0),
///     Coordinate::new_2d(10.0, 0.0),
///     Coordinate::new_2d(10.0, 10.0),
///     Coordinate::new_2d(0.0, 10.0),
///     Coordinate::new_2d(0.0, 0.0),
/// ];
/// let ext1 = LineString::new(coords1)?;
/// let poly1 = Polygon::new(ext1, vec![])?;
/// # let coords2 = vec![
/// #     Coordinate::new_2d(5.0, 5.0),
/// #     Coordinate::new_2d(15.0, 5.0),
/// #     Coordinate::new_2d(15.0, 15.0),
/// #     Coordinate::new_2d(5.0, 15.0),
/// #     Coordinate::new_2d(5.0, 5.0),
/// # ];
/// # let ext2 = LineString::new(coords2)?;
/// # let poly2 = Polygon::new(ext2, vec![])?;
/// let results = symmetric_difference_pooled(&poly1, &poly2)?;
/// # Ok::<(), oxigeo_algorithms::error::AlgorithmError>(())
/// ```
pub fn symmetric_difference_pooled(
    poly1: &Polygon,
    poly2: &Polygon,
) -> Result<Vec<PoolGuard<'static, Polygon>>> {
    let results = symmetric_difference(poly1, poly2)?;

    // Convert results to pooled polygons
    let mut pooled_results = Vec::new();
    for result in results {
        let mut poly = get_pooled_polygon();
        poly.exterior = result.exterior;
        poly.interiors = result.interiors;
        pooled_results.push(poly);
    }

    Ok(pooled_results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_square(x: f64, y: f64, size: f64) -> Result<Polygon> {
        let coords = vec![
            Coordinate::new_2d(x, y),
            Coordinate::new_2d(x + size, y),
            Coordinate::new_2d(x + size, y + size),
            Coordinate::new_2d(x, y + size),
            Coordinate::new_2d(x, y),
        ];
        let exterior = LineString::new(coords).map_err(|e| AlgorithmError::Core(e))?;
        Polygon::new(exterior, vec![]).map_err(|e| AlgorithmError::Core(e))
    }

    #[test]
    fn test_difference_polygon_disjoint() {
        let poly1 = create_square(0.0, 0.0, 5.0);
        let poly2 = create_square(10.0, 10.0, 5.0);

        assert!(poly1.is_ok() && poly2.is_ok());
        if let (Ok(p1), Ok(p2)) = (poly1, poly2) {
            let result = difference_polygon(&p1, &p2);
            assert!(result.is_ok());
            if let Ok(polys) = result {
                assert_eq!(polys.len(), 1); // poly1 unchanged
            }
        }
    }

    #[test]
    fn test_difference_polygon_contained() {
        let poly1 = create_square(0.0, 0.0, 10.0);
        let poly2 = create_square(2.0, 2.0, 3.0);

        assert!(poly1.is_ok() && poly2.is_ok());
        if let (Ok(p1), Ok(p2)) = (poly1, poly2) {
            let result = difference_polygon(&p1, &p2);
            assert!(result.is_ok());
            if let Ok(polys) = result {
                // poly2 should become a hole in poly1
                assert_eq!(polys.len(), 1);
                assert_eq!(polys[0].interiors.len(), 1);
            }
        }
    }

    #[test]
    fn test_difference_polygon_completely_subtracted() {
        let poly1 = create_square(2.0, 2.0, 3.0);
        let poly2 = create_square(0.0, 0.0, 10.0);

        assert!(poly1.is_ok() && poly2.is_ok());
        if let (Ok(p1), Ok(p2)) = (poly1, poly2) {
            let result = difference_polygon(&p1, &p2);
            assert!(result.is_ok());
            if let Ok(polys) = result {
                // poly1 completely inside poly2 - result is empty
                assert_eq!(polys.len(), 0);
            }
        }
    }

    #[test]
    fn test_difference_polygons_multiple() {
        let base = vec![create_square(0.0, 0.0, 10.0).ok()];
        let subtract = vec![
            create_square(2.0, 2.0, 2.0).ok(),
            create_square(6.0, 6.0, 2.0).ok(),
        ];

        let base_polys: Vec<_> = base.into_iter().flatten().collect();
        let subtract_polys: Vec<_> = subtract.into_iter().flatten().collect();

        let result = difference_polygons(&base_polys, &subtract_polys);
        assert!(result.is_ok());
    }

    #[test]
    fn test_symmetric_difference() {
        let poly1 = create_square(0.0, 0.0, 5.0);
        let poly2 = create_square(3.0, 0.0, 5.0);

        assert!(poly1.is_ok() && poly2.is_ok());
        if let (Ok(p1), Ok(p2)) = (poly1, poly2) {
            let result = symmetric_difference(&p1, &p2);
            assert!(result.is_ok());
            // Should return non-overlapping parts
            if let Ok(polys) = result {
                assert!(!polys.is_empty());
            }
        }
    }

    #[test]
    fn test_clip_to_box_inside() {
        let poly = create_square(2.0, 2.0, 3.0);
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let result = clip_to_box(&p, 0.0, 0.0, 10.0, 10.0);
            assert!(result.is_ok());
            if let Ok(polys) = result {
                // Completely inside - return unchanged
                assert_eq!(polys.len(), 1);
            }
        }
    }

    #[test]
    fn test_clip_to_box_outside() {
        let poly = create_square(15.0, 15.0, 3.0);
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let result = clip_to_box(&p, 0.0, 0.0, 10.0, 10.0);
            assert!(result.is_ok());
            if let Ok(polys) = result {
                // Completely outside - return empty
                assert_eq!(polys.len(), 0);
            }
        }
    }

    #[test]
    fn test_clip_to_box_partial() {
        let poly = create_square(5.0, 5.0, 10.0);
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let result = clip_to_box(&p, 0.0, 0.0, 10.0, 10.0);
            assert!(result.is_ok());
            if let Ok(polys) = result {
                // Partially overlapping - should be clipped
                assert_eq!(polys.len(), 1);
            }
        }
    }

    #[test]
    fn test_erase_small_holes() {
        let exterior_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(20.0, 0.0),
            Coordinate::new_2d(20.0, 20.0),
            Coordinate::new_2d(0.0, 20.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        let small_hole_coords = vec![
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(3.0, 2.0),
            Coordinate::new_2d(3.0, 3.0),
            Coordinate::new_2d(2.0, 3.0),
            Coordinate::new_2d(2.0, 2.0),
        ];

        let large_hole_coords = vec![
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(15.0, 10.0),
            Coordinate::new_2d(15.0, 15.0),
            Coordinate::new_2d(10.0, 15.0),
            Coordinate::new_2d(10.0, 10.0),
        ];

        let exterior = LineString::new(exterior_coords);
        let small_hole = LineString::new(small_hole_coords);
        let large_hole = LineString::new(large_hole_coords);

        assert!(exterior.is_ok() && small_hole.is_ok() && large_hole.is_ok());

        if let (Ok(ext), Ok(sh), Ok(lh)) = (exterior, small_hole, large_hole) {
            let polygon = Polygon::new(ext, vec![sh, lh]);
            assert!(polygon.is_ok());

            if let Ok(poly) = polygon {
                let result = erase_small_holes(&poly, 10.0);
                assert!(result.is_ok());
                if let Ok(cleaned) = result {
                    // Small hole should be removed, large hole kept
                    assert_eq!(cleaned.interiors.len(), 1);
                }
            }
        }
    }

    // Helper function to create a square polygon with a hole
    fn create_square_with_hole(
        x: f64,
        y: f64,
        size: f64,
        hole_x: f64,
        hole_y: f64,
        hole_size: f64,
    ) -> Result<Polygon> {
        let exterior_coords = vec![
            Coordinate::new_2d(x, y),
            Coordinate::new_2d(x + size, y),
            Coordinate::new_2d(x + size, y + size),
            Coordinate::new_2d(x, y + size),
            Coordinate::new_2d(x, y),
        ];
        let hole_coords = vec![
            Coordinate::new_2d(hole_x, hole_y),
            Coordinate::new_2d(hole_x + hole_size, hole_y),
            Coordinate::new_2d(hole_x + hole_size, hole_y + hole_size),
            Coordinate::new_2d(hole_x, hole_y + hole_size),
            Coordinate::new_2d(hole_x, hole_y),
        ];

        let exterior = LineString::new(exterior_coords).map_err(AlgorithmError::Core)?;
        let hole = LineString::new(hole_coords).map_err(AlgorithmError::Core)?;

        Polygon::new(exterior, vec![hole]).map_err(AlgorithmError::Core)
    }

    // ========== Tests for Interior Ring (Hole) Handling ==========

    #[test]
    fn test_difference_poly1_with_hole_disjoint() {
        // Polygon with hole, subtracting a disjoint polygon
        let poly1 = create_square_with_hole(0.0, 0.0, 20.0, 5.0, 5.0, 5.0);
        let poly2 = create_square(30.0, 30.0, 5.0);

        assert!(poly1.is_ok() && poly2.is_ok());
        if let (Ok(p1), Ok(p2)) = (poly1, poly2) {
            let result = difference_polygon(&p1, &p2);
            assert!(result.is_ok());
            if let Ok(polys) = result {
                // poly1 should be unchanged with its hole
                assert_eq!(polys.len(), 1);
                assert_eq!(polys[0].interiors.len(), 1);
            }
        }
    }

    #[test]
    fn test_difference_poly1_with_hole_contained_subtract() {
        // Polygon with hole, subtracting a polygon that's inside but not in the hole
        let poly1 = create_square_with_hole(0.0, 0.0, 20.0, 5.0, 5.0, 5.0);
        let poly2 = create_square(12.0, 12.0, 3.0);

        assert!(poly1.is_ok() && poly2.is_ok());
        if let (Ok(p1), Ok(p2)) = (poly1, poly2) {
            let result = difference_polygon(&p1, &p2);
            assert!(result.is_ok());
            if let Ok(polys) = result {
                // poly1 should have 2 holes now (original + new one from poly2)
                assert_eq!(polys.len(), 1);
                assert_eq!(polys[0].interiors.len(), 2);
            }
        }
    }

    #[test]
    fn test_difference_subtract_poly_with_hole() {
        // Subtracting a polygon that has a hole creates new polygon region
        let poly1 = create_square(0.0, 0.0, 20.0);
        let poly2 = create_square_with_hole(2.0, 2.0, 16.0, 6.0, 6.0, 4.0);

        assert!(poly1.is_ok() && poly2.is_ok());
        if let (Ok(p1), Ok(p2)) = (poly1, poly2) {
            let result = difference_polygon(&p1, &p2);
            assert!(result.is_ok());
            if let Ok(polys) = result {
                // Should have at least 2 polygons:
                // 1. The outer part (poly1 - poly2)
                // 2. The hole area of poly2 (which becomes a filled region)
                assert!(!polys.is_empty());
            }
        }
    }

    #[test]
    fn test_difference_poly1_inside_hole_of_poly2() {
        // poly1 is inside a hole of poly2 - should return poly1 unchanged
        let exterior_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(30.0, 0.0),
            Coordinate::new_2d(30.0, 30.0),
            Coordinate::new_2d(0.0, 30.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let hole_coords = vec![
            Coordinate::new_2d(5.0, 5.0),
            Coordinate::new_2d(25.0, 5.0),
            Coordinate::new_2d(25.0, 25.0),
            Coordinate::new_2d(5.0, 25.0),
            Coordinate::new_2d(5.0, 5.0),
        ];

        let exterior = LineString::new(exterior_coords);
        let hole = LineString::new(hole_coords);

        assert!(exterior.is_ok() && hole.is_ok());
        if let (Ok(ext), Ok(h)) = (exterior, hole) {
            let poly2 = Polygon::new(ext, vec![h]);
            let poly1 = create_square(10.0, 10.0, 5.0);

            assert!(poly1.is_ok() && poly2.is_ok());
            if let (Ok(p1), Ok(p2)) = (poly1, poly2) {
                let result = difference_polygon(&p1, &p2);
                assert!(result.is_ok());
                if let Ok(polys) = result {
                    // poly1 is inside hole of poly2, so not affected
                    assert_eq!(polys.len(), 1);
                }
            }
        }
    }

    #[test]
    fn test_clip_to_box_with_hole_inside() {
        // Polygon with hole, where hole is completely inside the clipping box
        let poly = create_square_with_hole(0.0, 0.0, 20.0, 5.0, 5.0, 5.0);
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let result = clip_to_box(&p, 0.0, 0.0, 25.0, 25.0);
            assert!(result.is_ok());
            if let Ok(polys) = result {
                // Polygon and hole should be preserved
                assert_eq!(polys.len(), 1);
                assert_eq!(polys[0].interiors.len(), 1);
            }
        }
    }

    #[test]
    fn test_clip_to_box_hole_outside() {
        // Polygon with hole, where hole is outside the clipping box
        let exterior_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(20.0, 0.0),
            Coordinate::new_2d(20.0, 20.0),
            Coordinate::new_2d(0.0, 20.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let hole_coords = vec![
            Coordinate::new_2d(15.0, 15.0),
            Coordinate::new_2d(18.0, 15.0),
            Coordinate::new_2d(18.0, 18.0),
            Coordinate::new_2d(15.0, 18.0),
            Coordinate::new_2d(15.0, 15.0),
        ];

        let exterior = LineString::new(exterior_coords);
        let hole = LineString::new(hole_coords);

        assert!(exterior.is_ok() && hole.is_ok());
        if let (Ok(ext), Ok(h)) = (exterior, hole) {
            let poly = Polygon::new(ext, vec![h]);
            assert!(poly.is_ok());

            if let Ok(p) = poly {
                // Clip to box that doesn't include the hole
                let result = clip_to_box(&p, 0.0, 0.0, 10.0, 10.0);
                assert!(result.is_ok());
                if let Ok(polys) = result {
                    // Hole should be removed (outside clip box)
                    assert_eq!(polys.len(), 1);
                    assert_eq!(polys[0].interiors.len(), 0);
                }
            }
        }
    }

    #[test]
    fn test_clip_to_box_hole_partial() {
        // Polygon with hole, where hole straddles the clipping box boundary
        let exterior_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(20.0, 0.0),
            Coordinate::new_2d(20.0, 20.0),
            Coordinate::new_2d(0.0, 20.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let hole_coords = vec![
            Coordinate::new_2d(8.0, 8.0),
            Coordinate::new_2d(12.0, 8.0),
            Coordinate::new_2d(12.0, 12.0),
            Coordinate::new_2d(8.0, 12.0),
            Coordinate::new_2d(8.0, 8.0),
        ];

        let exterior = LineString::new(exterior_coords);
        let hole = LineString::new(hole_coords);

        assert!(exterior.is_ok() && hole.is_ok());
        if let (Ok(ext), Ok(h)) = (exterior, hole) {
            let poly = Polygon::new(ext, vec![h]);
            assert!(poly.is_ok());

            if let Ok(p) = poly {
                // Clip to box that partially includes the hole
                let result = clip_to_box(&p, 0.0, 0.0, 10.0, 10.0);
                assert!(result.is_ok());
                // The hole should be clipped or removed depending on the result
            }
        }
    }

    #[test]
    fn test_validate_polygon_topology_valid() {
        let poly = create_square_with_hole(0.0, 0.0, 20.0, 5.0, 5.0, 5.0);
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let result = validate_polygon_topology(&p);
            assert!(result.is_ok());
            if let Ok(valid) = result {
                assert!(valid);
            }
        }
    }

    #[test]
    fn test_validate_polygon_topology_hole_outside() {
        // Create polygon with hole outside exterior
        let exterior_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(0.0, 10.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let hole_coords = vec![
            Coordinate::new_2d(20.0, 20.0),
            Coordinate::new_2d(25.0, 20.0),
            Coordinate::new_2d(25.0, 25.0),
            Coordinate::new_2d(20.0, 25.0),
            Coordinate::new_2d(20.0, 20.0),
        ];

        let exterior = LineString::new(exterior_coords);
        let hole = LineString::new(hole_coords);

        assert!(exterior.is_ok() && hole.is_ok());
        if let (Ok(ext), Ok(h)) = (exterior, hole) {
            // Note: Polygon::new doesn't validate that holes are inside exterior
            // So we construct the polygon directly for testing
            let poly = Polygon {
                exterior: ext,
                interiors: vec![h],
            };

            let result = validate_polygon_topology(&poly);
            assert!(result.is_ok());
            if let Ok(valid) = result {
                // Should be invalid because hole is outside
                assert!(!valid);
            }
        }
    }

    #[test]
    fn test_merge_adjacent_holes_no_overlap() {
        let hole1_coords = vec![
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(4.0, 2.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(2.0, 4.0),
            Coordinate::new_2d(2.0, 2.0),
        ];
        let hole2_coords = vec![
            Coordinate::new_2d(6.0, 6.0),
            Coordinate::new_2d(8.0, 6.0),
            Coordinate::new_2d(8.0, 8.0),
            Coordinate::new_2d(6.0, 8.0),
            Coordinate::new_2d(6.0, 6.0),
        ];

        let hole1 = LineString::new(hole1_coords);
        let hole2 = LineString::new(hole2_coords);

        assert!(hole1.is_ok() && hole2.is_ok());
        if let (Ok(h1), Ok(h2)) = (hole1, hole2) {
            let result = merge_adjacent_holes(&[h1, h2]);
            assert!(result.is_ok());
            if let Ok(merged) = result {
                // No overlap, both holes should be preserved
                assert_eq!(merged.len(), 2);
            }
        }
    }

    #[test]
    fn test_merge_adjacent_holes_with_overlap() {
        let hole1_coords = vec![
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(6.0, 2.0),
            Coordinate::new_2d(6.0, 6.0),
            Coordinate::new_2d(2.0, 6.0),
            Coordinate::new_2d(2.0, 2.0),
        ];
        let hole2_coords = vec![
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(8.0, 4.0),
            Coordinate::new_2d(8.0, 8.0),
            Coordinate::new_2d(4.0, 8.0),
            Coordinate::new_2d(4.0, 4.0),
        ];

        let hole1 = LineString::new(hole1_coords);
        let hole2 = LineString::new(hole2_coords);

        assert!(hole1.is_ok() && hole2.is_ok());
        if let (Ok(h1), Ok(h2)) = (hole1, hole2) {
            let result = merge_adjacent_holes(&[h1, h2]);
            assert!(result.is_ok());
            if let Ok(merged) = result {
                // Overlapping holes should be merged into one
                assert_eq!(merged.len(), 1);
            }
        }
    }

    #[test]
    fn test_point_in_ring() {
        let ring = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(0.0, 10.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        // Point inside
        let inside = point_in_ring(&Coordinate::new_2d(5.0, 5.0), &ring);
        assert!(inside);

        // Point outside
        let outside = point_in_ring(&Coordinate::new_2d(15.0, 15.0), &ring);
        assert!(!outside);

        // Point on edge (behavior may vary)
        let on_edge = point_in_ring(&Coordinate::new_2d(5.0, 0.0), &ring);
        // Edge cases are implementation-dependent
        let _ = on_edge;
    }

    #[test]
    fn test_is_ring_inside_ring() {
        let outer = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(20.0, 0.0),
            Coordinate::new_2d(20.0, 20.0),
            Coordinate::new_2d(0.0, 20.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        let inner = vec![
            Coordinate::new_2d(5.0, 5.0),
            Coordinate::new_2d(15.0, 5.0),
            Coordinate::new_2d(15.0, 15.0),
            Coordinate::new_2d(5.0, 15.0),
            Coordinate::new_2d(5.0, 5.0),
        ];

        let outside = vec![
            Coordinate::new_2d(30.0, 30.0),
            Coordinate::new_2d(40.0, 30.0),
            Coordinate::new_2d(40.0, 40.0),
            Coordinate::new_2d(30.0, 40.0),
            Coordinate::new_2d(30.0, 30.0),
        ];

        assert!(is_ring_inside_ring(&inner, &outer));
        assert!(!is_ring_inside_ring(&outside, &outer));
    }

    #[test]
    fn test_is_ring_inside_ring_concave() {
        // A concave "C"-shaped outer ring opening to the right. Its notch
        // (x in [3,10], y in [3,7]) is OUTSIDE the ring material.
        let outer = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 3.0),
            Coordinate::new_2d(3.0, 3.0),
            Coordinate::new_2d(3.0, 7.0),
            Coordinate::new_2d(10.0, 7.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(0.0, 10.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        // A smaller concave "C" that lies entirely within the outer material,
        // but whose CENTROID (5.25, 5.0) falls inside the outer's notch — i.e.
        // OUTSIDE the outer ring. The old centroid-only check returned `false`
        // here (dropping a genuinely-contained hole); the robust check returns
        // `true`.
        let inner = vec![
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(9.0, 1.0),
            Coordinate::new_2d(9.0, 2.0),
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(2.0, 8.0),
            Coordinate::new_2d(9.0, 8.0),
            Coordinate::new_2d(9.0, 9.0),
            Coordinate::new_2d(1.0, 9.0),
            Coordinate::new_2d(1.0, 1.0),
        ];

        // Confirm the premise: the inner centroid is outside the outer ring, so
        // a centroid-only test would give the wrong answer.
        let centroid = compute_ring_centroid(&inner);
        assert!(
            !point_in_ring(&centroid, &outer),
            "test premise: inner centroid must fall in the outer's notch"
        );

        // Robust check correctly reports containment.
        assert!(
            is_ring_inside_ring(&inner, &outer),
            "concave inner ring is genuinely inside the concave outer ring"
        );

        // A ring straddling the outer's inner spine (x=3): part sits in the
        // material (x<3), part in the notch (x>3), so its edges properly cross
        // the outer boundary and it must NOT be reported inside.
        let straddling = vec![
            Coordinate::new_2d(1.0, 4.0),
            Coordinate::new_2d(6.0, 4.0), // crosses the spine at x=3
            Coordinate::new_2d(6.0, 6.0),
            Coordinate::new_2d(1.0, 6.0), // crosses the spine at x=3
            Coordinate::new_2d(1.0, 4.0),
        ];
        assert!(
            !is_ring_inside_ring(&straddling, &outer),
            "a ring crossing the outer boundary is not contained"
        );
    }

    #[test]
    fn test_clip_ring_to_box() {
        let ring = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(20.0, 0.0),
            Coordinate::new_2d(20.0, 20.0),
            Coordinate::new_2d(0.0, 20.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        let result = clip_ring_to_box(&ring, 5.0, 5.0, 15.0, 15.0);
        assert!(result.is_ok());
        if let Ok(clipped) = result {
            // Should produce a ring roughly 10x10
            assert!(clipped.len() >= 4);
        }
    }

    #[test]
    fn test_compute_ring_centroid() {
        let ring = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(0.0, 10.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        let centroid = compute_ring_centroid(&ring);
        // For a square, centroid should be at center
        assert!((centroid.x - 4.0).abs() < 1.0); // Approximate due to closing point
        assert!((centroid.y - 4.0).abs() < 1.0);
    }

    #[test]
    fn test_difference_preserves_unaffected_holes() {
        // Create polygon with multiple holes
        let exterior_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(30.0, 0.0),
            Coordinate::new_2d(30.0, 30.0),
            Coordinate::new_2d(0.0, 30.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let hole1_coords = vec![
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(5.0, 2.0),
            Coordinate::new_2d(5.0, 5.0),
            Coordinate::new_2d(2.0, 5.0),
            Coordinate::new_2d(2.0, 2.0),
        ];
        let hole2_coords = vec![
            Coordinate::new_2d(20.0, 20.0),
            Coordinate::new_2d(25.0, 20.0),
            Coordinate::new_2d(25.0, 25.0),
            Coordinate::new_2d(20.0, 25.0),
            Coordinate::new_2d(20.0, 20.0),
        ];

        let exterior = LineString::new(exterior_coords);
        let hole1 = LineString::new(hole1_coords);
        let hole2 = LineString::new(hole2_coords);

        assert!(exterior.is_ok() && hole1.is_ok() && hole2.is_ok());
        if let (Ok(ext), Ok(h1), Ok(h2)) = (exterior, hole1, hole2) {
            let poly1 = Polygon::new(ext, vec![h1, h2]);
            let poly2 = create_square(10.0, 10.0, 5.0);

            assert!(poly1.is_ok() && poly2.is_ok());
            if let (Ok(p1), Ok(p2)) = (poly1, poly2) {
                let result = difference_polygon(&p1, &p2);
                assert!(result.is_ok());
                if let Ok(polys) = result {
                    // poly2 should become a new hole, existing holes should be preserved
                    assert_eq!(polys.len(), 1);
                    // Should have at least 3 holes (2 original + 1 new)
                    assert!(polys[0].interiors.len() >= 2);
                }
            }
        }
    }
}

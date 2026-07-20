//! Buffer generation for geometries
//!
//! This module implements robust geometric buffering operations that create
//! offset geometries around input features. Buffer operations are fundamental
//! in spatial analysis for proximity analysis, safety zones, and cartographic
//! generalization.
//!
//! # Implementation Notes
//!
//! The buffer algorithm uses parallel offset curves for linear geometries and
//! Minkowski sum principles for polygons. The implementation handles:
//!
//! - Different cap styles (round, flat, square) for line endpoints
//! - Different join styles (round, miter, bevel) for line vertices
//! - Negative buffers (erosion) for polygons
//! - Self-intersection resolution
//!
//! # Examples
//!
//! ```
//! use oxigdal_algorithms::vector::{buffer_point, Point, BufferOptions};
//!
//! let point = Point::new(0.0, 0.0);
//! let options = BufferOptions::default();
//! let result = buffer_point(&point, 10.0, &options);
//! ```

use crate::error::{AlgorithmError, Result};
use crate::vector::offset::{JoinStyle, OffsetOptions, offset_polygon_rings};
use crate::vector::pool::{PoolGuard, get_pooled_polygon};
use oxigdal_core::vector::{Coordinate, LineString, Point, Polygon};

#[cfg(not(feature = "std"))]
use core::f64::consts::PI;
#[cfg(feature = "std")]
use std::f64::consts::PI;

/// End cap style for line buffers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BufferCapStyle {
    /// Round caps (semi-circles at endpoints)
    #[default]
    Round,
    /// Flat caps (perpendicular to line direction)
    Flat,
    /// Square caps (extended by buffer distance)
    Square,
}

/// Join style for line buffers at vertices
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BufferJoinStyle {
    /// Round joins (circular arcs)
    #[default]
    Round,
    /// Miter joins (sharp points, with miter limit)
    Miter,
    /// Bevel joins (cut off at buffer distance)
    Bevel,
}

/// Options for buffer operations
#[derive(Debug, Clone)]
pub struct BufferOptions {
    /// Number of segments per quadrant for round caps/joins
    pub quadrant_segments: usize,
    /// Cap style for line endpoints
    pub cap_style: BufferCapStyle,
    /// Join style for line vertices
    pub join_style: BufferJoinStyle,
    /// Miter limit (ratio) for miter joins
    pub miter_limit: f64,
    /// Simplification tolerance (0.0 = no simplification)
    pub simplify_tolerance: f64,
}

impl Default for BufferOptions {
    fn default() -> Self {
        Self {
            quadrant_segments: 8,
            cap_style: BufferCapStyle::Round,
            join_style: BufferJoinStyle::Round,
            miter_limit: 5.0,
            simplify_tolerance: 0.0,
        }
    }
}

/// Generates a circular buffer around a point
///
/// # Arguments
///
/// * `center` - The center point
/// * `radius` - Buffer radius (must be positive)
/// * `options` - Buffer options controlling segment count and other parameters
///
/// # Errors
///
/// Returns error if radius is negative or non-finite
pub fn buffer_point(center: &Point, radius: f64, options: &BufferOptions) -> Result<Polygon> {
    if radius < 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "radius",
            message: "radius must be non-negative".to_string(),
        });
    }

    if !radius.is_finite() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "radius",
            message: "radius must be finite".to_string(),
        });
    }

    if radius == 0.0 {
        // Degenerate case: return point as tiny polygon
        return create_degenerate_polygon(&center.coord);
    }

    let segments = options.quadrant_segments * 4;
    let mut coords = Vec::with_capacity(segments + 1);

    for i in 0..segments {
        let angle = 2.0 * PI * (i as f64) / (segments as f64);
        let x = center.coord.x + radius * angle.cos();
        let y = center.coord.y + radius * angle.sin();
        coords.push(Coordinate::new_2d(x, y));
    }

    // Close the ring
    coords.push(coords[0]);

    let exterior = LineString::new(coords).map_err(AlgorithmError::Core)?;
    Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)
}

/// Generates a buffer around a linestring
///
/// Creates a polygon buffer around a linestring using parallel offset curves
/// on both sides, with configurable cap and join styles.
///
/// # Arguments
///
/// * `line` - The linestring to buffer
/// * `distance` - Buffer distance (positive for expansion, negative for contraction)
/// * `options` - Buffer options
///
/// # Errors
///
/// Returns error if linestring is invalid or has insufficient points
pub fn buffer_linestring(
    line: &LineString,
    distance: f64,
    options: &BufferOptions,
) -> Result<Polygon> {
    if line.coords.len() < 2 {
        return Err(AlgorithmError::InsufficientData {
            operation: "buffer_linestring",
            message: "linestring must have at least 2 coordinates".to_string(),
        });
    }

    if !distance.is_finite() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "distance",
            message: "distance must be finite".to_string(),
        });
    }

    if distance == 0.0 {
        // Degenerate case: return line as thin polygon
        return create_degenerate_linestring_polygon(line);
    }

    let abs_distance = distance.abs();
    let mut left_coords = Vec::new();
    let mut right_coords = Vec::new();

    // Generate parallel offset curves
    for i in 0..(line.coords.len() - 1) {
        let p1 = &line.coords[i];
        let p2 = &line.coords[i + 1];

        let (left, right) = offset_segment(p1, p2, abs_distance)?;

        if i == 0 {
            // Start cap
            add_start_cap(&mut left_coords, p1, &left, abs_distance, options);
        }

        left_coords.push(left);

        if i == line.coords.len() - 2 {
            // Last segment
            let (left2, right2) = offset_segment(p1, p2, abs_distance)?;
            left_coords.push(left2);

            // End cap
            add_end_cap(&mut left_coords, p2, &left2, abs_distance, options);

            // Add right side in reverse
            right_coords.insert(0, right2);
            right_coords.insert(0, right);
        } else {
            // Add join. `left` is the offset of the segment start point `p1`;
            // for a correct corner join we need the offset of the *vertex* `p2`
            // along this segment's normal. That is `p2 + (left - p1)`, since
            // `(left - p1)` is exactly the perpendicular offset vector of the
            // segment `(p1, p2)`. `left3` is already the vertex offset of `p2`
            // along the next segment's normal.
            let p3 = &line.coords[i + 2];
            let (left3, _) = offset_segment(p2, p3, abs_distance)?;
            let off1_at_vertex = Coordinate::new_2d(p2.x + (left.x - p1.x), p2.y + (left.y - p1.y));

            add_join(
                &mut left_coords,
                &off1_at_vertex,
                &left3,
                p2,
                abs_distance,
                options,
            )?;

            right_coords.insert(0, right);
        }
    }

    // Combine left and right sides
    left_coords.extend(right_coords);
    left_coords.push(left_coords[0]); // Close ring

    let exterior = LineString::new(left_coords).map_err(AlgorithmError::Core)?;
    Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)
}

/// Generates a buffer around a polygon
///
/// For positive distances, expands the polygon. For negative distances,
/// performs erosion (inward buffer).
///
/// # Arguments
///
/// * `polygon` - The polygon to buffer
/// * `distance` - Buffer distance (positive expands, negative erodes)
/// * `options` - Buffer options
///
/// # Errors
///
/// Returns error if polygon is invalid
pub fn buffer_polygon(
    polygon: &Polygon,
    distance: f64,
    options: &BufferOptions,
) -> Result<Polygon> {
    if !distance.is_finite() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "distance",
            message: "distance must be finite".to_string(),
        });
    }

    if distance == 0.0 {
        // No change
        return Ok(polygon.clone());
    }

    // For polygon buffering, we buffer the exterior ring outward
    // and interior rings inward (to expand holes)
    let exterior_buffer = buffer_ring(&polygon.exterior, distance, options, false)?;

    // Handle interior rings (holes)
    let mut interior_buffers = Vec::new();
    for interior in &polygon.interiors {
        // Invert distance for holes
        let hole_buffer = buffer_ring(interior, -distance, options, true)?;
        interior_buffers.push(hole_buffer);
    }

    Polygon::new(exterior_buffer, interior_buffers).map_err(AlgorithmError::Core)
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Creates a degenerate polygon from a single point
fn create_degenerate_polygon(coord: &Coordinate) -> Result<Polygon> {
    let coords = vec![*coord, *coord, *coord, *coord];
    let exterior = LineString::new(coords).map_err(AlgorithmError::Core)?;
    Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)
}

/// Creates a degenerate polygon from a linestring (collapsed)
fn create_degenerate_linestring_polygon(line: &LineString) -> Result<Polygon> {
    let mut coords = line.coords.clone();
    coords.reverse();
    coords.extend_from_slice(&line.coords);
    coords.push(coords[0]);

    let exterior = LineString::new(coords).map_err(AlgorithmError::Core)?;
    Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)
}

/// Computes offset points for a line segment
///
/// Returns (left_offset, right_offset) perpendicular to the segment direction
fn offset_segment(
    p1: &Coordinate,
    p2: &Coordinate,
    distance: f64,
) -> Result<(Coordinate, Coordinate)> {
    let dx = p2.x - p1.x;
    let dy = p2.y - p1.y;
    let length = (dx * dx + dy * dy).sqrt();

    if length < f64::EPSILON {
        return Err(AlgorithmError::GeometryError {
            message: "degenerate segment (zero length)".to_string(),
        });
    }

    // Perpendicular vector (rotated 90 degrees)
    let perp_x = -dy / length;
    let perp_y = dx / length;

    let left = Coordinate::new_2d(p1.x + perp_x * distance, p1.y + perp_y * distance);

    let right = Coordinate::new_2d(p1.x - perp_x * distance, p1.y - perp_y * distance);

    Ok((left, right))
}

/// Adds a start cap to the buffer
fn add_start_cap(
    coords: &mut Vec<Coordinate>,
    point: &Coordinate,
    offset: &Coordinate,
    distance: f64,
    options: &BufferOptions,
) {
    match options.cap_style {
        BufferCapStyle::Round => {
            add_round_cap(coords, point, offset, distance, options, true);
        }
        BufferCapStyle::Flat => {
            coords.push(*offset);
        }
        BufferCapStyle::Square => {
            // Extend by distance in direction perpendicular to offset
            let dx = offset.x - point.x;
            let dy = offset.y - point.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len > f64::EPSILON {
                let nx = -dy / len;
                let ny = dx / len;
                let extended =
                    Coordinate::new_2d(offset.x + nx * distance, offset.y + ny * distance);
                coords.push(extended);
            }
            coords.push(*offset);
        }
    }
}

/// Adds an end cap to the buffer
fn add_end_cap(
    coords: &mut Vec<Coordinate>,
    point: &Coordinate,
    offset: &Coordinate,
    distance: f64,
    options: &BufferOptions,
) {
    match options.cap_style {
        BufferCapStyle::Round => {
            add_round_cap(coords, point, offset, distance, options, false);
        }
        BufferCapStyle::Flat => {
            coords.push(*offset);
        }
        BufferCapStyle::Square => {
            let dx = offset.x - point.x;
            let dy = offset.y - point.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len > f64::EPSILON {
                let nx = dy / len;
                let ny = -dx / len;
                let extended =
                    Coordinate::new_2d(offset.x + nx * distance, offset.y + ny * distance);
                coords.push(*offset);
                coords.push(extended);
            }
        }
    }
}

/// Adds a round cap (semi-circle)
fn add_round_cap(
    coords: &mut Vec<Coordinate>,
    center: &Coordinate,
    start_offset: &Coordinate,
    radius: f64,
    options: &BufferOptions,
    is_start: bool,
) {
    let segments = options.quadrant_segments * 2; // Half circle
    let start_angle = (start_offset.y - center.y).atan2(start_offset.x - center.x);

    for i in 0..=segments {
        let t = if is_start {
            (i as f64) / (segments as f64)
        } else {
            (i as f64) / (segments as f64)
        };
        let angle = start_angle + t * PI * if is_start { 1.0 } else { -1.0 };
        let x = center.x + radius * angle.cos();
        let y = center.y + radius * angle.sin();
        coords.push(Coordinate::new_2d(x, y));
    }
}

/// Adds a join between two offset segments
fn add_join(
    coords: &mut Vec<Coordinate>,
    offset1: &Coordinate,
    offset2: &Coordinate,
    vertex: &Coordinate,
    distance: f64,
    options: &BufferOptions,
) -> Result<()> {
    match options.join_style {
        BufferJoinStyle::Round => {
            add_round_join(coords, offset1, offset2, vertex, distance, options);
        }
        BufferJoinStyle::Miter => {
            add_miter_join(coords, offset1, offset2, vertex, distance, options)?;
        }
        BufferJoinStyle::Bevel => {
            coords.push(*offset1);
            coords.push(*offset2);
        }
    }
    Ok(())
}

/// Adds a round join (circular arc)
fn add_round_join(
    coords: &mut Vec<Coordinate>,
    offset1: &Coordinate,
    offset2: &Coordinate,
    center: &Coordinate,
    radius: f64,
    options: &BufferOptions,
) {
    coords.push(*offset1);

    let angle1 = (offset1.y - center.y).atan2(offset1.x - center.x);
    let angle2 = (offset2.y - center.y).atan2(offset2.x - center.x);

    let mut angle_diff = angle2 - angle1;
    // Normalize to [-PI, PI]
    while angle_diff > PI {
        angle_diff -= 2.0 * PI;
    }
    while angle_diff < -PI {
        angle_diff += 2.0 * PI;
    }

    let segments = ((angle_diff.abs() / (PI / 2.0)) * (options.quadrant_segments as f64)) as usize;

    for i in 1..segments {
        let t = (i as f64) / (segments as f64);
        let angle = angle1 + t * angle_diff;
        let x = center.x + radius * angle.cos();
        let y = center.y + radius * angle.sin();
        coords.push(Coordinate::new_2d(x, y));
    }
}

/// Adds a miter join (sharp corner with limit)
fn add_miter_join(
    coords: &mut Vec<Coordinate>,
    offset1: &Coordinate,
    offset2: &Coordinate,
    vertex: &Coordinate,
    distance: f64,
    options: &BufferOptions,
) -> Result<()> {
    coords.push(*offset1);

    // Compute miter point (intersection of two offset lines)
    // If miter is too sharp, fall back to bevel
    let miter_result = compute_miter_point(offset1, offset2, vertex, distance, options.miter_limit);

    if let Some(miter) = miter_result {
        coords.push(miter);
    }

    coords.push(*offset2);
    Ok(())
}

/// Computes the true miter join point.
///
/// The miter point is the intersection of the two offset lines, which lies on
/// the corner bisector at distance `distance / cos(θ/2)` from the vertex, where
/// θ is the turn angle. It is generally *beyond* both offset points (farther
/// from the vertex), so that the sharp outer corner is fully covered — this is
/// what distinguishes a miter join from a bevel join.
///
/// `offset1` and `offset2` must be the offset points *at the vertex* (each at
/// distance `distance` from `vertex`), so that `(offset - vertex) / distance`
/// recovers the corresponding segment's unit normal. This mirrors the correct
/// `emit_miter` implementation in [`crate::vector::offset`].
///
/// Returns `None` (caller falls back to bevel) when the corner is (near)
/// straight/anti-parallel, or when the miter extension ratio exceeds
/// `miter_limit`.
fn compute_miter_point(
    offset1: &Coordinate,
    offset2: &Coordinate,
    vertex: &Coordinate,
    distance: f64,
    miter_limit: f64,
) -> Option<Coordinate> {
    if distance.abs() < f64::EPSILON {
        return None;
    }

    // Arm vectors from the vertex to each offset point (length ≈ `distance`).
    let ax = offset1.x - vertex.x;
    let ay = offset1.y - vertex.y;
    let bx = offset2.x - vertex.x;
    let by = offset2.y - vertex.y;

    // Bisector direction = normalize(na + nb) where na = a/distance, nb = b/distance.
    let sx = ax + bx;
    let sy = ay + by;
    let blen = (sx * sx + sy * sy).sqrt();

    // Collinear / anti-parallel normals → no distinct miter point; use bevel.
    if blen < f64::EPSILON {
        return None;
    }

    let bux = sx / blen;
    let buy = sy / blen;

    // cos(θ/2) = bisector_unit · na, where na = a / distance.
    let cos_half = (bux * ax + buy * ay) / distance;
    if cos_half.abs() < f64::EPSILON {
        // Nearly anti-parallel → miter length blows up; fall back to bevel.
        return None;
    }

    // Signed miter length along the bisector.
    let miter_length = distance / cos_half;
    let ratio = (miter_length / distance).abs();

    if ratio > miter_limit.abs() || !miter_length.is_finite() {
        // Too sharp: fall back to bevel.
        None
    } else {
        Some(Coordinate::new_2d(
            vertex.x + miter_length * bux,
            vertex.y + miter_length * buy,
        ))
    }
}

/// Buffers a closed ring (for polygon buffering).
///
/// This offsets the ring outward for a positive `distance` and inward for a
/// negative `distance`, applying the configured [`BufferJoinStyle`] at every
/// vertex. Orientation (CW/CCW) is detected internally via the shoelace
/// formula so a positive `distance` always expands an exterior (CCW) ring
/// *outward* regardless of the input winding. The heavy lifting is delegated to
/// the correct closed-ring offset machinery in [`crate::vector::offset`], which
/// inserts proper miter/bevel/round join geometry at each corner rather than
/// dropping the corners entirely.
///
/// `is_hole` is retained for API symmetry; the outward/inward direction for
/// holes is already encoded by the caller negating `distance`.
fn buffer_ring(
    ring: &LineString,
    distance: f64,
    options: &BufferOptions,
    _is_hole: bool,
) -> Result<LineString> {
    if ring.coords.len() < 4 {
        return Err(AlgorithmError::InsufficientData {
            operation: "buffer_ring",
            message: "ring must have at least 4 coordinates".to_string(),
        });
    }

    // Convert the ring to the tuple representation used by the offset engine.
    let ring_tuples: Vec<(f64, f64)> = ring.coords.iter().map(|c| (c.x, c.y)).collect();

    // Translate the buffer options to offset options so the requested join
    // style (and miter limit / simplification) is honoured for polygons.
    let offset_options = OffsetOptions {
        miter_limit: options.miter_limit,
        join_style: match options.join_style {
            BufferJoinStyle::Round => JoinStyle::Round,
            BufferJoinStyle::Miter => JoinStyle::Miter,
            BufferJoinStyle::Bevel => JoinStyle::Bevel,
        },
        simplify_tolerance: if options.simplify_tolerance > 0.0 {
            Some(options.simplify_tolerance)
        } else {
            None
        },
    };

    let mut offset_rings = offset_polygon_rings(&[ring_tuples], distance, &offset_options)?;

    let out_ring = offset_rings
        .pop()
        .ok_or_else(|| AlgorithmError::GeometryError {
            message: "offset_polygon_rings returned no ring".to_string(),
        })?;

    let offset_coords: Vec<Coordinate> = out_ring
        .iter()
        .map(|&(x, y)| Coordinate::new_2d(x, y))
        .collect();

    LineString::new(offset_coords).map_err(AlgorithmError::Core)
}

//
// Pooled buffer operations for reduced allocations
//

/// Generates a circular buffer around a point using object pooling
///
/// This is the pooled version of `buffer_point` that reuses allocated
/// polygons from a thread-local pool, reducing allocation overhead for
/// batch operations.
///
/// # Arguments
///
/// * `center` - The center point
/// * `radius` - Buffer radius (must be positive)
/// * `options` - Buffer options controlling segment count and other parameters
///
/// # Returns
///
/// A `PoolGuard<Polygon>` that automatically returns the polygon to the pool
/// when dropped. Use `.into_inner()` to take ownership without returning to pool.
///
/// # Errors
///
/// Returns error if radius is negative or non-finite
///
/// # Performance
///
/// For batch operations, this can reduce allocations by 2-3x compared to
/// the non-pooled version.
///
/// # Example
///
/// ```
/// use oxigdal_algorithms::vector::{buffer_point_pooled, Point, BufferOptions};
///
/// let point = Point::new(0.0, 0.0);
/// let options = BufferOptions::default();
/// let buffered = buffer_point_pooled(&point, 10.0, &options)?;
/// // Use buffered polygon...
/// // Automatically returned to pool when buffered drops
/// # Ok::<(), oxigdal_algorithms::error::AlgorithmError>(())
/// ```
pub fn buffer_point_pooled(
    center: &Point,
    radius: f64,
    options: &BufferOptions,
) -> Result<PoolGuard<'static, Polygon>> {
    if radius < 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "radius",
            message: "radius must be non-negative".to_string(),
        });
    }

    if !radius.is_finite() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "radius",
            message: "radius must be finite".to_string(),
        });
    }

    let mut poly = get_pooled_polygon();

    if radius == 0.0 {
        // Degenerate case: return point as tiny polygon
        let degenerate = create_degenerate_polygon(&center.coord)?;
        poly.exterior = degenerate.exterior;
        poly.interiors = degenerate.interiors;
        return Ok(poly);
    }

    let segments = options.quadrant_segments * 4;
    poly.exterior.coords.clear();
    poly.exterior.coords.reserve(segments + 1);

    for i in 0..segments {
        let angle = 2.0 * PI * (i as f64) / (segments as f64);
        let x = center.coord.x + radius * angle.cos();
        let y = center.coord.y + radius * angle.sin();
        poly.exterior.coords.push(Coordinate::new_2d(x, y));
    }

    // Close the ring
    if let Some(&first) = poly.exterior.coords.first() {
        poly.exterior.coords.push(first);
    }

    Ok(poly)
}

/// Generates a buffer around a linestring using object pooling
///
/// This is the pooled version of `buffer_linestring` that reuses allocated
/// polygons from a thread-local pool.
///
/// # Arguments
///
/// * `line` - The linestring to buffer
/// * `distance` - Buffer distance (positive for expansion)
/// * `options` - Buffer options
///
/// # Returns
///
/// A `PoolGuard<Polygon>` that automatically returns the polygon to the pool
/// when dropped.
///
/// # Errors
///
/// Returns error if linestring is invalid or has insufficient points
///
/// # Example
///
/// ```
/// use oxigdal_algorithms::vector::{buffer_linestring_pooled, LineString, Coordinate, BufferOptions};
///
/// let coords = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(10.0, 0.0)];
/// let line = LineString::new(coords)?;
/// let options = BufferOptions::default();
/// let buffered = buffer_linestring_pooled(&line, 5.0, &options)?;
/// # Ok::<(), oxigdal_algorithms::error::AlgorithmError>(())
/// ```
pub fn buffer_linestring_pooled(
    line: &LineString,
    distance: f64,
    options: &BufferOptions,
) -> Result<PoolGuard<'static, Polygon>> {
    // Compute the buffer using the non-pooled version
    let result = buffer_linestring(line, distance, options)?;

    // Get a pooled polygon and copy the result into it
    let mut poly = get_pooled_polygon();
    poly.exterior = result.exterior;
    poly.interiors = result.interiors;

    Ok(poly)
}

/// Generates a buffer around a polygon using object pooling
///
/// This is the pooled version of `buffer_polygon` that reuses allocated
/// polygons from a thread-local pool.
///
/// # Arguments
///
/// * `polygon` - The polygon to buffer
/// * `distance` - Buffer distance (positive for expansion, negative for erosion)
/// * `options` - Buffer options
///
/// # Returns
///
/// A `PoolGuard<Polygon>` that automatically returns the polygon to the pool
/// when dropped.
///
/// # Errors
///
/// Returns error if polygon is invalid
///
/// # Example
///
/// ```
/// use oxigdal_algorithms::vector::{buffer_polygon_pooled, Polygon, LineString, Coordinate, BufferOptions};
///
/// let exterior = LineString::new(vec![
///     Coordinate::new_2d(0.0, 0.0),
///     Coordinate::new_2d(10.0, 0.0),
///     Coordinate::new_2d(10.0, 10.0),
///     Coordinate::new_2d(0.0, 10.0),
///     Coordinate::new_2d(0.0, 0.0),
/// ])?;
/// let polygon = Polygon::new(exterior, vec![])?;
/// let options = BufferOptions::default();
/// let buffered = buffer_polygon_pooled(&polygon, 2.0, &options)?;
/// # Ok::<(), oxigdal_algorithms::error::AlgorithmError>(())
/// ```
pub fn buffer_polygon_pooled(
    polygon: &Polygon,
    distance: f64,
    options: &BufferOptions,
) -> Result<PoolGuard<'static, Polygon>> {
    // Compute the buffer using the non-pooled version
    let result = buffer_polygon(polygon, distance, options)?;

    // Get a pooled polygon and copy the result into it
    let mut poly = get_pooled_polygon();
    poly.exterior = result.exterior;
    poly.interiors = result.interiors;

    Ok(poly)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_buffer_point_basic() {
        let point = Point::new(0.0, 0.0);
        let options = BufferOptions::default();
        let result = buffer_point(&point, 10.0, &options);
        assert!(result.is_ok());

        let polygon = result.ok();
        assert!(polygon.is_some());
        if let Some(poly) = polygon {
            // Check that all points are approximately at distance 10 from center
            for coord in &poly.exterior.coords {
                let dist = (coord.x * coord.x + coord.y * coord.y).sqrt();
                assert_relative_eq!(dist, 10.0, epsilon = 1e-10);
            }
        }
    }

    #[test]
    fn test_buffer_point_zero_radius() {
        let point = Point::new(5.0, 5.0);
        let options = BufferOptions::default();
        let result = buffer_point(&point, 0.0, &options);
        assert!(result.is_ok());
    }

    #[test]
    fn test_buffer_point_negative_radius() {
        let point = Point::new(0.0, 0.0);
        let options = BufferOptions::default();
        let result = buffer_point(&point, -10.0, &options);
        assert!(result.is_err());
    }

    #[test]
    fn test_buffer_linestring_basic() {
        let coords = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(10.0, 0.0)];
        let line = LineString::new(coords);
        assert!(line.is_ok());

        if let Ok(ls) = line {
            let options = BufferOptions::default();
            let result = buffer_linestring(&ls, 5.0, &options);
            assert!(result.is_ok());

            if let Ok(poly) = result {
                // Buffer should create a polygon
                assert!(poly.exterior.coords.len() > 4);
            }
        }
    }

    #[test]
    fn test_buffer_linestring_empty() {
        let coords = vec![Coordinate::new_2d(0.0, 0.0)];
        let line = LineString::new(coords);
        assert!(line.is_err()); // Should fail in LineString::new
    }

    #[test]
    fn test_buffer_polygon_basic() {
        let exterior_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(0.0, 10.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(exterior_coords);
        assert!(exterior.is_ok());

        if let Ok(ext) = exterior {
            let polygon = Polygon::new(ext, vec![]);
            assert!(polygon.is_ok());

            if let Ok(poly) = polygon {
                let options = BufferOptions::default();
                let result = buffer_polygon(&poly, 2.0, &options);
                assert!(result.is_ok());

                // The buffered exterior must EXPAND outward, not shrink inward:
                // a unit-square [0,10]×[0,10] buffered by +2 must reach roughly
                // [-2,12]×[-2,12]. The old (buggy) implementation produced points
                // strictly inside the original bounding box.
                if let Ok(buffered) = result {
                    let mut min_x = f64::INFINITY;
                    let mut min_y = f64::INFINITY;
                    let mut max_x = f64::NEG_INFINITY;
                    let mut max_y = f64::NEG_INFINITY;
                    for c in &buffered.exterior.coords {
                        min_x = min_x.min(c.x);
                        min_y = min_y.min(c.y);
                        max_x = max_x.max(c.x);
                        max_y = max_y.max(c.y);
                    }
                    // Strictly grows beyond the original [0,10]×[0,10] box.
                    assert!(min_x < 0.0, "min_x should extend below 0, got {min_x}");
                    assert!(min_y < 0.0, "min_y should extend below 0, got {min_y}");
                    assert!(max_x > 10.0, "max_x should extend beyond 10, got {max_x}");
                    assert!(max_y > 10.0, "max_y should extend beyond 10, got {max_y}");
                    // Should be close to the expected [-2, 12] extent.
                    assert!((-2.5..=-1.5).contains(&min_x), "min_x ~ -2, got {min_x}");
                    assert!((11.5..=12.5).contains(&max_x), "max_x ~ 12, got {max_x}");
                }
            }
        }
    }

    #[test]
    fn test_buffer_polygon_honors_join_style() {
        // A right-angle square exercises corner join geometry. Each join style
        // must be honoured (previously `_options` was ignored for polygons).
        let exterior_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(0.0, 10.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(exterior_coords).expect("valid ring");
        let poly = Polygon::new(exterior, vec![]).expect("valid polygon");

        let mut round_pts = 0usize;
        let mut bevel_pts = 0usize;
        let mut miter_pts = 0usize;

        for (style, out) in [
            (BufferJoinStyle::Round, &mut round_pts),
            (BufferJoinStyle::Bevel, &mut bevel_pts),
            (BufferJoinStyle::Miter, &mut miter_pts),
        ] {
            let options = BufferOptions {
                join_style: style,
                ..BufferOptions::default()
            };
            let buffered = buffer_polygon(&poly, 2.0, &options).expect("buffer ok");
            // All coordinates must be finite and the ring closed.
            for c in &buffered.exterior.coords {
                assert!(c.x.is_finite() && c.y.is_finite());
            }
            *out = buffered.exterior.coords.len();
        }

        // Round joins insert arc points at each corner, so they must produce
        // strictly more vertices than the sharp miter join.
        assert!(
            round_pts > miter_pts,
            "round joins ({round_pts}) should add more points than miter ({miter_pts})"
        );
        // Bevel inserts two points per corner; miter a single point per corner,
        // so bevel must have at least as many points as miter.
        assert!(
            bevel_pts >= miter_pts,
            "bevel ({bevel_pts}) should have >= miter ({miter_pts}) points"
        );
    }

    #[test]
    fn test_compute_miter_point_extends_beyond_offsets() {
        // Right-angle corner at the vertex (10, 0). The two vertex offsets are
        // on the offset lines y = 1 (through (10,1)) and x = 9 (through (9,0)),
        // so the true miter (their intersection) is (9, 1) — NOT the midpoint
        // (9.5, 0.5) that the old implementation returned.
        let vertex = Coordinate::new_2d(10.0, 0.0);
        let offset1 = Coordinate::new_2d(10.0, 1.0); // seg1 (east) left-normal
        let offset2 = Coordinate::new_2d(9.0, 0.0); // seg2 (north) left-normal
        let miter =
            compute_miter_point(&offset1, &offset2, &vertex, 1.0, 5.0).expect("within miter limit");
        assert_relative_eq!(miter.x, 9.0, epsilon = 1e-9);
        assert_relative_eq!(miter.y, 1.0, epsilon = 1e-9);

        // Explicitly reject the degenerate midpoint answer.
        let midpoint_x = (offset1.x + offset2.x) / 2.0;
        let midpoint_y = (offset1.y + offset2.y) / 2.0;
        assert!(
            (miter.x - midpoint_x).abs() > 0.1 || (miter.y - midpoint_y).abs() > 0.1,
            "miter point must differ from the offset midpoint"
        );
    }

    #[test]
    fn test_compute_miter_point_exceeds_limit_falls_back() {
        // Nearly anti-parallel arms → miter blows up → None (bevel fallback).
        let vertex = Coordinate::new_2d(0.0, 0.0);
        let offset1 = Coordinate::new_2d(0.0, 1.0);
        // A tiny tilt away from straight-down: cos(θ/2) ≈ 0.005 → ratio ≈ 200.
        let offset2 = Coordinate::new_2d(0.01, -0.99995);
        let miter = compute_miter_point(&offset1, &offset2, &vertex, 1.0, 5.0);
        assert!(
            miter.is_none(),
            "sharp corner must fall back to bevel (None), got {miter:?}"
        );
    }

    #[test]
    fn test_offset_segment() {
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(10.0, 0.0);
        let result = offset_segment(&p1, &p2, 5.0);

        assert!(result.is_ok());
        if let Ok((left, right)) = result {
            assert_relative_eq!(left.x, 0.0, epsilon = 1e-10);
            assert_relative_eq!(left.y, 5.0, epsilon = 1e-10);
            assert_relative_eq!(right.x, 0.0, epsilon = 1e-10);
            assert_relative_eq!(right.y, -5.0, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_buffer_cap_styles() {
        let coords = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(10.0, 0.0)];
        let line = LineString::new(coords);
        assert!(line.is_ok());

        if let Ok(ls) = line {
            // Test round caps
            let mut options = BufferOptions::default();
            options.cap_style = BufferCapStyle::Round;
            let result = buffer_linestring(&ls, 5.0, &options);
            assert!(result.is_ok());

            // Test flat caps
            options.cap_style = BufferCapStyle::Flat;
            let result = buffer_linestring(&ls, 5.0, &options);
            assert!(result.is_ok());

            // Test square caps
            options.cap_style = BufferCapStyle::Square;
            let result = buffer_linestring(&ls, 5.0, &options);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_buffer_join_styles() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 10.0),
        ];
        let line = LineString::new(coords);
        assert!(line.is_ok());

        if let Ok(ls) = line {
            // Test round joins
            let mut options = BufferOptions::default();
            options.join_style = BufferJoinStyle::Round;
            let result = buffer_linestring(&ls, 5.0, &options);
            assert!(result.is_ok());

            // Test miter joins
            options.join_style = BufferJoinStyle::Miter;
            let result = buffer_linestring(&ls, 5.0, &options);
            assert!(result.is_ok());

            // Test bevel joins
            options.join_style = BufferJoinStyle::Bevel;
            let result = buffer_linestring(&ls, 5.0, &options);
            assert!(result.is_ok());
        }
    }
}

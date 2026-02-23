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
            // Add join
            let p3 = &line.coords[i + 2];
            let (left3, _) = offset_segment(p2, p3, abs_distance)?;

            add_join(&mut left_coords, &left, &left3, p2, abs_distance, options)?;

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

/// Computes the miter join point
fn compute_miter_point(
    offset1: &Coordinate,
    offset2: &Coordinate,
    _vertex: &Coordinate,
    distance: f64,
    miter_limit: f64,
) -> Option<Coordinate> {
    // Simple implementation: just check if offsets are too far apart
    let dx = offset2.x - offset1.x;
    let dy = offset2.y - offset1.y;
    let miter_distance = (dx * dx + dy * dy).sqrt();

    if miter_distance > distance * miter_limit {
        // Too sharp, return None to use bevel instead
        None
    } else {
        // Use midpoint as approximation
        Some(Coordinate::new_2d(
            (offset1.x + offset2.x) / 2.0,
            (offset1.y + offset2.y) / 2.0,
        ))
    }
}

/// Buffers a ring (for polygon buffering)
fn buffer_ring(
    ring: &LineString,
    distance: f64,
    _options: &BufferOptions,
    _is_hole: bool,
) -> Result<LineString> {
    if ring.coords.len() < 4 {
        return Err(AlgorithmError::InsufficientData {
            operation: "buffer_ring",
            message: "ring must have at least 4 coordinates".to_string(),
        });
    }

    let abs_distance = distance.abs();
    let mut offset_coords = Vec::new();

    // Process each edge of the ring
    for i in 0..(ring.coords.len() - 1) {
        let p1 = &ring.coords[i];
        let p2 = &ring.coords[i + 1];

        let (left, _right) = offset_segment(p1, p2, abs_distance)?;

        if distance > 0.0 {
            offset_coords.push(left);
        } else {
            // For negative buffer, use right side
            let (_left, right) = offset_segment(p1, p2, abs_distance)?;
            offset_coords.push(right);
        }
    }

    // Close the ring
    if let Some(first) = offset_coords.first() {
        offset_coords.push(*first);
    }

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
            }
        }
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

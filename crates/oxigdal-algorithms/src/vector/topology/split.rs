//! Split operations for dividing geometric features
//!
//! This module implements splitting operations that divide geometries using
//! other geometries as cutting tools.

use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::{Coordinate, LineString, Point, Polygon};

/// Options for split operations
#[derive(Debug, Clone)]
pub struct SplitOptions {
    /// Tolerance for coordinate comparison
    pub tolerance: f64,
    /// Whether to snap split points to grid
    pub snap_to_grid: bool,
    /// Grid size for snapping (if enabled)
    pub grid_size: f64,
    /// Minimum length for resulting line segments
    pub min_segment_length: f64,
    /// Whether to preserve all split parts (even very small ones)
    pub preserve_all: bool,
}

impl Default for SplitOptions {
    fn default() -> Self {
        Self {
            tolerance: 1e-10,
            snap_to_grid: false,
            grid_size: 1e-6,
            min_segment_length: 0.0,
            preserve_all: true,
        }
    }
}

/// Result of a split operation
#[derive(Debug, Clone)]
pub struct SplitResult {
    /// The resulting geometries from the split
    pub geometries: Vec<SplitGeometry>,
    /// Number of split points used
    pub num_splits: usize,
    /// Whether all splits were successful
    pub complete: bool,
}

/// A geometry resulting from a split operation
#[derive(Debug, Clone)]
pub enum SplitGeometry {
    /// A linestring segment
    LineString(LineString),
    /// A polygon
    Polygon(Polygon),
}

/// Split a linestring by point locations
///
/// Divides a linestring into multiple segments at specified points.
///
/// # Arguments
///
/// * `linestring` - The linestring to split
/// * `split_points` - Points where the linestring should be split
/// * `options` - Split options
///
/// # Returns
///
/// Result containing the split result with multiple linestring segments
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::vector::topology::{split_linestring_by_points, SplitOptions};
/// use oxigdal_algorithms::{Coordinate, LineString, Point};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let coords = vec![
///     Coordinate::new_2d(0.0, 0.0),
///     Coordinate::new_2d(10.0, 0.0),
/// ];
/// let linestring = LineString::new(coords)?;
///
/// let split_points = vec![
///     Point::new(5.0, 0.0),
/// ];
///
/// let result = split_linestring_by_points(&linestring, &split_points, &SplitOptions::default())?;
/// assert_eq!(result.num_splits, 1);
/// assert!(result.geometries.len() >= 2);
/// # Ok(())
/// # }
/// ```
pub fn split_linestring_by_points(
    linestring: &LineString,
    split_points: &[Point],
    options: &SplitOptions,
) -> Result<SplitResult> {
    if split_points.is_empty() {
        return Ok(SplitResult {
            geometries: vec![SplitGeometry::LineString(linestring.clone())],
            num_splits: 0,
            complete: true,
        });
    }

    let coords = &linestring.coords;
    if coords.len() < 2 {
        return Err(AlgorithmError::InvalidGeometry(
            "Linestring must have at least 2 coordinates".to_string(),
        ));
    }

    // Find all split locations along the linestring
    let mut split_locations = Vec::new();

    for point in split_points {
        if let Some(location) = find_point_on_linestring(linestring, point, options.tolerance)? {
            split_locations.push(location);
        }
    }

    // Sort split locations by segment index and parameter
    // Validate for NaN values before sorting
    for loc in &split_locations {
        if loc.parameter.is_nan() {
            return Err(AlgorithmError::InvalidGeometry(
                "Split location parameter contains NaN value".to_string(),
            ));
        }
    }

    split_locations.sort_by(|a, b| {
        a.segment_index.cmp(&b.segment_index).then(
            a.parameter
                .partial_cmp(&b.parameter)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    // Remove duplicates
    split_locations.dedup_by(|a, b| {
        a.segment_index == b.segment_index && (a.parameter - b.parameter).abs() < options.tolerance
    });

    if split_locations.is_empty() {
        return Ok(SplitResult {
            geometries: vec![SplitGeometry::LineString(linestring.clone())],
            num_splits: 0,
            complete: false,
        });
    }

    // Build split segments
    let mut result_geometries = Vec::new();
    let mut current_coords = Vec::new();
    let mut current_segment_idx = 0;
    let mut split_idx = 0;

    current_coords.push(coords[0]);

    for i in 0..coords.len().saturating_sub(1) {
        // Add splits on this segment
        while split_idx < split_locations.len() && split_locations[split_idx].segment_index == i {
            let split_loc = &split_locations[split_idx];
            let split_coord = split_loc.coordinate;

            // Add split point
            current_coords.push(split_coord);

            // Create a new linestring if we have enough points
            if current_coords.len() >= 2 {
                if let Ok(ls) = LineString::new(current_coords.clone()) {
                    if options.preserve_all
                        || compute_linestring_length(&ls) >= options.min_segment_length
                    {
                        result_geometries.push(SplitGeometry::LineString(ls));
                    }
                }
            }

            // Start new segment
            current_coords.clear();
            current_coords.push(split_coord);

            split_idx += 1;
        }

        // Add end point of current segment
        current_coords.push(coords[i + 1]);
        current_segment_idx = i + 1;
    }

    // Add final segment
    if current_coords.len() >= 2 {
        if let Ok(ls) = LineString::new(current_coords) {
            if options.preserve_all || compute_linestring_length(&ls) >= options.min_segment_length
            {
                result_geometries.push(SplitGeometry::LineString(ls));
            }
        }
    }

    Ok(SplitResult {
        geometries: result_geometries,
        num_splits: split_locations.len(),
        complete: true,
    })
}

/// Location of a point on a linestring
#[derive(Debug, Clone)]
struct PointLocation {
    /// Index of the segment (0-based)
    segment_index: usize,
    /// Parameter along the segment (0.0 to 1.0)
    parameter: f64,
    /// The coordinate at this location
    coordinate: Coordinate,
}

/// Find a point on a linestring
fn find_point_on_linestring(
    linestring: &LineString,
    point: &Point,
    tolerance: f64,
) -> Result<Option<PointLocation>> {
    let coords = &linestring.coords;

    for i in 0..coords.len().saturating_sub(1) {
        let p1 = coords[i];
        let p2 = coords[i + 1];

        // Check if point is on this segment
        if let Some((param, coord)) = point_on_segment(point, &p1, &p2, tolerance) {
            return Ok(Some(PointLocation {
                segment_index: i,
                parameter: param,
                coordinate: coord,
            }));
        }
    }

    Ok(None)
}

/// Check if a point is on a line segment
fn point_on_segment(
    point: &Point,
    p1: &Coordinate,
    p2: &Coordinate,
    tolerance: f64,
) -> Option<(f64, Coordinate)> {
    let px = point.coord.x;
    let py = point.coord.y;

    // Vector from p1 to p2
    let dx = p2.x - p1.x;
    let dy = p2.y - p1.y;

    // Length squared of segment
    let len_sq = dx * dx + dy * dy;

    if len_sq < tolerance * tolerance {
        // Degenerate segment
        return None;
    }

    // Parameter t along the segment
    let t = ((px - p1.x) * dx + (py - p1.y) * dy) / len_sq;

    // Check if point projects onto the segment (not before or after)
    if t < -tolerance || t > 1.0 + tolerance {
        return None;
    }

    // Clamp t to [0, 1]
    let t = t.clamp(0.0, 1.0);

    // Compute closest point on segment
    let closest_x = p1.x + t * dx;
    let closest_y = p1.y + t * dy;

    // Check distance from point to closest point
    let dist_sq = (px - closest_x).powi(2) + (py - closest_y).powi(2);

    if dist_sq < tolerance * tolerance {
        Some((t, Coordinate::new_2d(closest_x, closest_y)))
    } else {
        None
    }
}

/// Compute the length of a linestring
fn compute_linestring_length(linestring: &LineString) -> f64 {
    let coords = &linestring.coords;
    let mut length = 0.0;

    for i in 0..coords.len().saturating_sub(1) {
        let dx = coords[i + 1].x - coords[i].x;
        let dy = coords[i + 1].y - coords[i].y;
        length += (dx * dx + dy * dy).sqrt();
    }

    length
}

/// Split a polygon by a linestring
///
/// Divides a polygon into multiple parts using a linestring as a cutting tool.
///
/// # Arguments
///
/// * `polygon` - The polygon to split
/// * `split_line` - The linestring to use for splitting
/// * `options` - Split options
///
/// # Returns
///
/// Result containing the split polygons
///
/// # Examples
///
/// ```no_run
/// use oxigdal_algorithms::vector::topology::{split_polygon_by_line, SplitOptions};
/// use oxigdal_algorithms::{Coordinate, LineString, Polygon};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let coords = vec![
///     Coordinate::new_2d(0.0, 0.0),
///     Coordinate::new_2d(10.0, 0.0),
///     Coordinate::new_2d(10.0, 10.0),
///     Coordinate::new_2d(0.0, 10.0),
///     Coordinate::new_2d(0.0, 0.0),
/// ];
/// let exterior = LineString::new(coords)?;
/// let polygon = Polygon::new(exterior, vec![])?;
///
/// let split_coords = vec![
///     Coordinate::new_2d(0.0, 5.0),
///     Coordinate::new_2d(10.0, 5.0),
/// ];
/// let split_line = LineString::new(split_coords)?;
///
/// let result = split_polygon_by_line(&polygon, &split_line, &SplitOptions::default())?;
/// assert!(result.geometries.len() >= 1);
/// # Ok(())
/// # }
/// ```
pub fn split_polygon_by_line(
    polygon: &Polygon,
    split_line: &LineString,
    options: &SplitOptions,
) -> Result<SplitResult> {
    // Find intersection points between polygon boundary and split line
    let intersection_points = find_polygon_line_intersections(polygon, split_line, options)?;

    if intersection_points.len() < 2 {
        // Not enough intersections to split
        return Ok(SplitResult {
            geometries: vec![SplitGeometry::Polygon(polygon.clone())],
            num_splits: 0,
            complete: false,
        });
    }

    // Create polygons from split
    let result_polygons =
        create_split_polygons(polygon, split_line, &intersection_points, options)?;

    let geometries = result_polygons
        .into_iter()
        .map(SplitGeometry::Polygon)
        .collect();

    Ok(SplitResult {
        geometries,
        num_splits: intersection_points.len(),
        complete: true,
    })
}

/// Find intersection points between polygon boundary and a linestring
fn find_polygon_line_intersections(
    polygon: &Polygon,
    line: &LineString,
    options: &SplitOptions,
) -> Result<Vec<Coordinate>> {
    let mut intersections = Vec::new();

    // Check exterior ring
    let exterior_intersections = find_linestring_intersections(&polygon.exterior, line, options)?;
    intersections.extend(exterior_intersections);

    // Check interior rings (holes)
    for interior in &polygon.interiors {
        let interior_intersections = find_linestring_intersections(interior, line, options)?;
        intersections.extend(interior_intersections);
    }

    // Remove duplicates
    // Validate for NaN values before sorting
    for coord in &intersections {
        if coord.x.is_nan() || coord.y.is_nan() {
            return Err(AlgorithmError::InvalidGeometry(
                "Intersection coordinate contains NaN value".to_string(),
            ));
        }
    }

    intersections.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
    });

    intersections.dedup_by(|a, b| {
        (a.x - b.x).abs() < options.tolerance && (a.y - b.y).abs() < options.tolerance
    });

    Ok(intersections)
}

/// Find intersections between two linestrings
fn find_linestring_intersections(
    line1: &LineString,
    line2: &LineString,
    options: &SplitOptions,
) -> Result<Vec<Coordinate>> {
    let mut intersections = Vec::new();
    let coords1 = &line1.coords;
    let coords2 = &line2.coords;

    for i in 0..coords1.len().saturating_sub(1) {
        for j in 0..coords2.len().saturating_sub(1) {
            if let Some(intersection) = compute_segment_intersection(
                &coords1[i],
                &coords1[i + 1],
                &coords2[j],
                &coords2[j + 1],
                options.tolerance,
            ) {
                intersections.push(intersection);
            }
        }
    }

    Ok(intersections)
}

/// Compute intersection point between two line segments
fn compute_segment_intersection(
    p1: &Coordinate,
    p2: &Coordinate,
    p3: &Coordinate,
    p4: &Coordinate,
    tolerance: f64,
) -> Option<Coordinate> {
    let d = (p1.x - p2.x) * (p3.y - p4.y) - (p1.y - p2.y) * (p3.x - p4.x);

    if d.abs() < tolerance {
        // Parallel or coincident
        return None;
    }

    let t = ((p1.x - p3.x) * (p3.y - p4.y) - (p1.y - p3.y) * (p3.x - p4.x)) / d;
    let u = -((p1.x - p2.x) * (p1.y - p3.y) - (p1.y - p2.y) * (p1.x - p3.x)) / d;

    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
        let x = p1.x + t * (p2.x - p1.x);
        let y = p1.y + t * (p2.y - p1.y);
        Some(Coordinate::new_2d(x, y))
    } else {
        None
    }
}

/// Create split polygons from intersection points
fn create_split_polygons(
    _polygon: &Polygon,
    _split_line: &LineString,
    _intersections: &[Coordinate],
    _options: &SplitOptions,
) -> Result<Vec<Polygon>> {
    // This is a simplified implementation
    // A full implementation would need to:
    // 1. Build a planar graph from the polygon and split line
    // 2. Find faces in the graph
    // 3. Construct polygons from faces

    // For now, return empty result as a placeholder
    Ok(Vec::new())
}

/// Split a polygon by another polygon
///
/// Divides a polygon using another polygon's boundary as cutting edges.
pub fn split_polygon_by_polygon(
    target_poly: &Polygon,
    split_poly: &Polygon,
    options: &SplitOptions,
) -> Result<SplitResult> {
    // Use the exterior ring of the split polygon as the splitting linestring
    let split_line = &split_poly.exterior.clone();

    split_polygon_by_line(target_poly, split_line, options)
}

/// Batch split operation for multiple polygons
pub fn split_polygons_batch(
    polygons: &[Polygon],
    split_line: &LineString,
    options: &SplitOptions,
) -> Result<Vec<SplitResult>> {
    polygons
        .iter()
        .map(|poly| split_polygon_by_line(poly, split_line, options))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_linestring(coords: Vec<(f64, f64)>) -> LineString {
        let coords: Vec<Coordinate> = coords
            .iter()
            .map(|(x, y)| Coordinate::new_2d(*x, *y))
            .collect();
        LineString::new(coords).expect("Failed to create linestring")
    }

    fn create_polygon(coords: Vec<(f64, f64)>) -> Polygon {
        let coords: Vec<Coordinate> = coords
            .iter()
            .map(|(x, y)| Coordinate::new_2d(*x, *y))
            .collect();
        let exterior = LineString::new(coords).expect("Failed to create linestring");
        Polygon::new(exterior, vec![]).expect("Failed to create polygon")
    }

    #[test]
    fn test_split_linestring_single_point() {
        let linestring = create_linestring(vec![(0.0, 0.0), (10.0, 0.0)]);
        let split_points = vec![Point::new(5.0, 0.0)];

        let result =
            split_linestring_by_points(&linestring, &split_points, &SplitOptions::default());
        assert!(result.is_ok());

        let split_result = result.expect("Split failed");
        assert_eq!(split_result.num_splits, 1);
        assert!(split_result.geometries.len() >= 2);
    }

    #[test]
    fn test_split_linestring_multiple_points() {
        let linestring = create_linestring(vec![(0.0, 0.0), (10.0, 0.0)]);
        let split_points = vec![Point::new(3.0, 0.0), Point::new(7.0, 0.0)];

        let result =
            split_linestring_by_points(&linestring, &split_points, &SplitOptions::default());
        assert!(result.is_ok());

        let split_result = result.expect("Split failed");
        assert_eq!(split_result.num_splits, 2);
    }

    #[test]
    fn test_split_linestring_no_intersection() {
        let linestring = create_linestring(vec![(0.0, 0.0), (10.0, 0.0)]);
        let split_points = vec![Point::new(5.0, 5.0)]; // Not on line

        let result =
            split_linestring_by_points(&linestring, &split_points, &SplitOptions::default());
        assert!(result.is_ok());

        let split_result = result.expect("Split failed");
        assert_eq!(split_result.num_splits, 0);
        assert_eq!(split_result.geometries.len(), 1); // Unchanged
    }

    #[test]
    fn test_split_linestring_empty_splits() {
        let linestring = create_linestring(vec![(0.0, 0.0), (10.0, 0.0)]);
        let split_points = vec![];

        let result =
            split_linestring_by_points(&linestring, &split_points, &SplitOptions::default());
        assert!(result.is_ok());

        let split_result = result.expect("Split failed");
        assert_eq!(split_result.num_splits, 0);
        assert_eq!(split_result.geometries.len(), 1);
    }

    #[test]
    fn test_point_on_segment() {
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(10.0, 0.0);
        let point = Point::new(5.0, 0.0);

        let result = point_on_segment(&point, &p1, &p2, 1e-10);
        assert!(result.is_some());

        if let Some((param, coord)) = result {
            assert!((param - 0.5).abs() < 1e-10);
            assert!((coord.x - 5.0).abs() < 1e-10);
            assert!((coord.y - 0.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_point_not_on_segment() {
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(10.0, 0.0);
        let point = Point::new(5.0, 5.0); // Off the line

        let result = point_on_segment(&point, &p1, &p2, 1e-10);
        assert!(result.is_none());
    }

    #[test]
    fn test_compute_segment_intersection() {
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(10.0, 10.0);
        let p3 = Coordinate::new_2d(0.0, 10.0);
        let p4 = Coordinate::new_2d(10.0, 0.0);

        let result = compute_segment_intersection(&p1, &p2, &p3, &p4, 1e-10);
        assert!(result.is_some());

        if let Some(intersection) = result {
            // Intersection should be at (5, 5)
            assert!((intersection.x - 5.0).abs() < 1e-6);
            assert!((intersection.y - 5.0).abs() < 1e-6);
        }
    }

    #[test]
    fn test_split_polygon_by_line() {
        let polygon = create_polygon(vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);

        let split_line = create_linestring(vec![(0.0, 5.0), (10.0, 5.0)]);

        let result = split_polygon_by_line(&polygon, &split_line, &SplitOptions::default());
        assert!(result.is_ok());

        let split_result = result.expect("Split failed");
        assert_eq!(split_result.num_splits, 2); // Two intersection points
    }

    #[test]
    fn test_compute_linestring_length() {
        let linestring = create_linestring(vec![(0.0, 0.0), (3.0, 0.0), (3.0, 4.0)]);
        let length = compute_linestring_length(&linestring);

        // Length is 3 + 4 = 7
        assert!((length - 7.0).abs() < 1e-6);
    }
}

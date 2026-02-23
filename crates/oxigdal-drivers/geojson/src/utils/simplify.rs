//! Geometry simplification utilities
//!
//! This module provides algorithms for simplifying geometries while
//! preserving their essential shape.

use crate::error::Result;
use crate::types::*;

/// Douglas-Peucker algorithm for line simplification
///
/// Simplifies a line by removing points that are within a given
/// tolerance distance from the simplified line.
pub fn douglas_peucker(coords: &[Position], tolerance: f64) -> Vec<Position> {
    if coords.len() <= 2 {
        return coords.to_vec();
    }

    // Find the point with maximum distance
    let mut max_distance = 0.0;
    let mut max_index = 0;

    let first = &coords[0];
    let last = &coords[coords.len() - 1];

    for (i, point) in coords.iter().enumerate().skip(1).take(coords.len() - 2) {
        let distance = perpendicular_distance(point, first, last);
        if distance > max_distance {
            max_distance = distance;
            max_index = i;
        }
    }

    // If max distance is greater than tolerance, recursively simplify
    if max_distance > tolerance {
        // Recursive call on both segments
        let mut left = douglas_peucker(&coords[..=max_index], tolerance);
        let right = douglas_peucker(&coords[max_index..], tolerance);

        // Remove duplicate point at junction
        left.pop();
        left.extend(right);
        left
    } else {
        // All points within tolerance, keep only endpoints
        vec![first.clone(), last.clone()]
    }
}

/// Computes perpendicular distance from a point to a line
fn perpendicular_distance(point: &Position, line_start: &Position, line_end: &Position) -> f64 {
    if point.len() < 2 || line_start.len() < 2 || line_end.len() < 2 {
        return 0.0;
    }

    let x = point[0];
    let y = point[1];
    let x1 = line_start[0];
    let y1 = line_start[1];
    let x2 = line_end[0];
    let y2 = line_end[1];

    let dx = x2 - x1;
    let dy = y2 - y1;

    if dx == 0.0 && dy == 0.0 {
        // Line start and end are the same point
        let dist_x = x - x1;
        let dist_y = y - y1;
        return (dist_x * dist_x + dist_y * dist_y).sqrt();
    }

    let numerator = (dy * x - dx * y + x2 * y1 - y2 * x1).abs();
    let denominator = (dx * dx + dy * dy).sqrt();

    numerator / denominator
}

/// Simplifies a LineString using Douglas-Peucker algorithm
pub fn simplify_linestring(linestring: &LineString, tolerance: f64) -> Result<LineString> {
    let simplified = douglas_peucker(&linestring.coordinates, tolerance);
    LineString::new(simplified)
}

/// Simplifies a Polygon using Douglas-Peucker algorithm
pub fn simplify_polygon(polygon: &Polygon, tolerance: f64) -> Result<Polygon> {
    let simplified_rings: Vec<_> = polygon
        .coordinates
        .iter()
        .map(|ring| douglas_peucker(ring, tolerance))
        .collect();

    Polygon::new(simplified_rings)
}

/// Simplifies a MultiLineString
pub fn simplify_multilinestring(mls: &MultiLineString, tolerance: f64) -> Result<MultiLineString> {
    let simplified_lines: Vec<_> = mls
        .coordinates
        .iter()
        .map(|line| douglas_peucker(line, tolerance))
        .collect();

    MultiLineString::new(simplified_lines)
}

/// Simplifies a MultiPolygon
pub fn simplify_multipolygon(mp: &MultiPolygon, tolerance: f64) -> Result<MultiPolygon> {
    let simplified_polygons: Vec<_> = mp
        .coordinates
        .iter()
        .map(|polygon| {
            polygon
                .iter()
                .map(|ring| douglas_peucker(ring, tolerance))
                .collect()
        })
        .collect();

    MultiPolygon::new(simplified_polygons)
}

/// Simplifies any Geometry
pub fn simplify_geometry(geometry: &Geometry, tolerance: f64) -> Result<Geometry> {
    match geometry {
        Geometry::Point(p) => Ok(Geometry::Point(p.clone())),
        Geometry::LineString(ls) => Ok(Geometry::LineString(simplify_linestring(ls, tolerance)?)),
        Geometry::Polygon(p) => Ok(Geometry::Polygon(simplify_polygon(p, tolerance)?)),
        Geometry::MultiPoint(mp) => Ok(Geometry::MultiPoint(mp.clone())),
        Geometry::MultiLineString(mls) => Ok(Geometry::MultiLineString(simplify_multilinestring(
            mls, tolerance,
        )?)),
        Geometry::MultiPolygon(mp) => Ok(Geometry::MultiPolygon(simplify_multipolygon(
            mp, tolerance,
        )?)),
        Geometry::GeometryCollection(gc) => {
            let simplified_geometries: Result<Vec<_>> = gc
                .geometries
                .iter()
                .map(|g| simplify_geometry(g, tolerance))
                .collect();
            Ok(Geometry::GeometryCollection(GeometryCollection::new(
                simplified_geometries?,
            )?))
        }
    }
}

/// Removes duplicate consecutive points from a coordinate sequence
pub fn remove_duplicates(coords: &[Position]) -> Vec<Position> {
    if coords.is_empty() {
        return Vec::new();
    }

    let mut result = vec![coords[0].clone()];

    for coord in coords.iter().skip(1) {
        if let Some(last) = result.last() {
            if !positions_equal(last, coord) {
                result.push(coord.clone());
            }
        }
    }

    result
}

/// Checks if two positions are equal
fn positions_equal(a: &Position, b: &Position) -> bool {
    if a.len() != b.len() {
        return false;
    }

    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-10)
}

/// Removes collinear points from a coordinate sequence
pub fn remove_collinear(coords: &[Position], tolerance: f64) -> Vec<Position> {
    if coords.len() <= 2 {
        return coords.to_vec();
    }

    let mut result = vec![coords[0].clone()];

    for i in 1..coords.len() - 1 {
        let prev = &coords[i - 1];
        let curr = &coords[i];
        let next = &coords[i + 1];

        let distance = perpendicular_distance(curr, prev, next);
        if distance > tolerance {
            result.push(curr.clone());
        }
    }

    result.push(coords[coords.len() - 1].clone());
    result
}

/// Computes the reduction ratio after simplification
pub fn simplification_ratio(original: &[Position], simplified: &[Position]) -> f64 {
    if original.is_empty() {
        return 0.0;
    }

    1.0 - (simplified.len() as f64 / original.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_douglas_peucker() {
        let coords = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.1],
            vec![2.0, -0.1],
            vec![3.0, 5.0],
            vec![4.0, 6.0],
            vec![5.0, 7.0],
            vec![6.0, 8.1],
            vec![7.0, 9.0],
            vec![8.0, 9.0],
            vec![9.0, 9.0],
        ];

        let simplified = douglas_peucker(&coords, 1.0);
        assert!(simplified.len() < coords.len());
    }

    #[test]
    fn test_simplify_linestring() {
        let coords = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.0],
            vec![2.0, 0.0],
            vec![3.0, 0.0],
        ];

        let ls = LineString::new(coords).expect("valid linestring");
        let simplified = simplify_linestring(&ls, 0.5).expect("simplification succeeded");

        assert!(simplified.len() < ls.len());
    }

    #[test]
    fn test_remove_duplicates() {
        let coords = vec![
            vec![0.0, 0.0],
            vec![0.0, 0.0],
            vec![1.0, 1.0],
            vec![1.0, 1.0],
            vec![2.0, 2.0],
        ];

        let cleaned = remove_duplicates(&coords);
        assert_eq!(cleaned.len(), 3);
    }

    #[test]
    fn test_remove_collinear() {
        let coords = vec![
            vec![0.0, 0.0],
            vec![1.0, 1.0],
            vec![2.0, 2.0],
            vec![3.0, 3.0],
        ];

        let cleaned = remove_collinear(&coords, 0.01);
        // Middle points are collinear and should be removed
        assert_eq!(cleaned.len(), 2);
    }

    #[test]
    fn test_simplification_ratio() {
        let original = vec![vec![0.0, 0.0]; 10];
        let simplified = vec![vec![0.0, 0.0]; 5];

        let ratio = simplification_ratio(&original, &simplified);
        assert_eq!(ratio, 0.5);
    }

    #[test]
    fn test_perpendicular_distance() {
        let point = vec![1.0, 1.0];
        let line_start = vec![0.0, 0.0];
        let line_end = vec![2.0, 0.0];

        let distance = perpendicular_distance(&point, &line_start, &line_end);
        assert!((distance - 1.0).abs() < 0.0001);
    }
}

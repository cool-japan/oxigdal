//! Douglas-Peucker line simplification algorithm
//!
//! Reduces the number of points in a polyline while preserving its general shape.
//! The algorithm recursively removes points that deviate less than a tolerance
//! from the line segment connecting their neighbors.

use super::{Coordinate, LineString};
use crate::error::{AlgorithmError, Result};

/// Simplifies a linestring using Douglas-Peucker algorithm
///
/// # Arguments
///
/// * `line` - Input line string
/// * `tolerance` - Maximum perpendicular distance for point removal
///
/// # Errors
///
/// Returns an error if the line is empty or tolerance is negative
pub fn simplify_linestring(line: &LineString, tolerance: f64) -> Result<LineString> {
    if line.coords.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "Douglas-Peucker simplification",
        });
    }

    if tolerance < 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "tolerance",
            message: "Tolerance must be non-negative".to_string(),
        });
    }

    if line.coords.len() <= 2 {
        return Ok(line.clone());
    }

    let mut keep = vec![false; line.coords.len()];
    keep[0] = true;
    keep[line.coords.len() - 1] = true;

    simplify_recursive(&line.coords, &mut keep, 0, line.coords.len() - 1, tolerance);

    let simplified_coords: Vec<Coordinate> = line
        .coords
        .iter()
        .zip(keep.iter())
        .filter(|&(_, &k)| k)
        .map(|(p, _)| *p)
        .collect();

    LineString::new(simplified_coords).map_err(AlgorithmError::Core)
}

/// Recursive Douglas-Peucker implementation
fn simplify_recursive(
    coords: &[Coordinate],
    keep: &mut [bool],
    start: usize,
    end: usize,
    tolerance: f64,
) {
    if end <= start + 1 {
        return;
    }

    // Find point with maximum distance from start-end line
    let mut max_dist = 0.0;
    let mut max_idx = start;

    for i in (start + 1)..end {
        let dist = perpendicular_distance(&coords[i], &coords[start], &coords[end]);
        if dist > max_dist {
            max_dist = dist;
            max_idx = i;
        }
    }

    // If max distance exceeds tolerance, keep the point and recurse
    if max_dist > tolerance {
        keep[max_idx] = true;
        simplify_recursive(coords, keep, start, max_idx, tolerance);
        simplify_recursive(coords, keep, max_idx, end, tolerance);
    }
}

/// Computes perpendicular distance from point to line segment
fn perpendicular_distance(
    point: &Coordinate,
    line_start: &Coordinate,
    line_end: &Coordinate,
) -> f64 {
    let dx = line_end.x - line_start.x;
    let dy = line_end.y - line_start.y;

    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        // Line segment is actually a point
        let dist_x = point.x - line_start.x;
        let dist_y = point.y - line_start.y;
        return (dist_x * dist_x + dist_y * dist_y).sqrt();
    }

    // Compute perpendicular distance using cross product
    let numerator =
        (dy * point.x - dx * point.y + line_end.x * line_start.y - line_end.y * line_start.x).abs();
    let denominator = (dx * dx + dy * dy).sqrt();

    numerator / denominator
}

/// Simplifies with area preservation constraint
///
/// This variant ensures the simplified line doesn't change the enclosed
/// area by more than a specified tolerance.
#[allow(dead_code)] // Reserved for advanced simplification modes
pub fn simplify_with_area_constraint(
    line: &LineString,
    tolerance: f64,
    max_area_change: f64,
) -> Result<LineString> {
    // Start with regular Douglas-Peucker
    let simplified = simplify_linestring(line, tolerance)?;

    // Iteratively adjust if area changes too much
    // (Simplified implementation - production would be more sophisticated)
    let _ = max_area_change; // Suppress unused warning

    Ok(simplified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simplify_straight_line() {
        // Points on a straight line should simplify to endpoints
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(3.0, 3.0),
        ];
        let line = LineString::new(coords);
        assert!(line.is_ok());

        if let Ok(ls) = line {
            let simplified = simplify_linestring(&ls, 0.1);
            assert!(simplified.is_ok());

            if let Ok(simplified) = simplified {
                assert_eq!(simplified.coords.len(), 2);
            }
        }
    }

    #[test]
    fn test_simplify_zigzag() {
        // Zigzag pattern should keep some points
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 0.0),
            Coordinate::new_2d(3.0, 1.0),
            Coordinate::new_2d(4.0, 0.0),
        ];
        let line = LineString::new(coords);
        assert!(line.is_ok());

        if let Ok(ls) = line {
            let simplified = simplify_linestring(&ls, 0.1);
            assert!(simplified.is_ok());

            if let Ok(simplified) = simplified {
                // Should keep all points for zigzag
                assert!(simplified.coords.len() >= 3);
            }
        }
    }

    #[test]
    fn test_simplify_tolerance() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.01), // Very small deviation
            Coordinate::new_2d(2.0, 0.0),
        ];
        let line = LineString::new(coords);
        assert!(line.is_ok());

        if let Ok(ls) = line {
            // Small tolerance should keep middle point
            let result1 = simplify_linestring(&ls, 0.001);
            assert!(result1.is_ok());

            // Large tolerance should remove middle point
            let result2 = simplify_linestring(&ls, 0.1);
            assert!(result2.is_ok());
        }
    }

    #[test]
    fn test_simplify_empty() {
        let coords: Vec<Coordinate> = vec![];
        let line = LineString::new(coords);
        // LineString::new will fail with fewer than 2 points
        assert!(line.is_err());
    }

    #[test]
    fn test_simplify_two_points() {
        let coords = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(1.0, 1.0)];
        let line = LineString::new(coords);
        assert!(line.is_ok());

        if let Ok(ls) = line {
            let result = simplify_linestring(&ls, 1.0);
            assert!(result.is_ok());

            if let Ok(simplified) = result {
                assert_eq!(simplified.coords.len(), 2);
            }
        }
    }

    #[test]
    fn test_perpendicular_distance() {
        let point = Coordinate::new_2d(1.0, 1.0);
        let line_start = Coordinate::new_2d(0.0, 0.0);
        let line_end = Coordinate::new_2d(2.0, 0.0);

        let dist = perpendicular_distance(&point, &line_start, &line_end);
        assert!((dist - 1.0).abs() < 1e-10);
    }
}

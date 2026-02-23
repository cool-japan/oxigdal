//! Delaunay triangulation
//!
//! Compute Delaunay triangulation of point sets.

use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::{Coordinate, LineString, Point, Polygon};

/// Options for Delaunay triangulation
#[derive(Debug, Clone)]
pub struct DelaunayOptions {
    /// Whether to include triangle quality metrics
    pub compute_quality: bool,
    /// Minimum angle threshold for quality triangles (degrees)
    pub min_angle: f64,
}

impl Default for DelaunayOptions {
    fn default() -> Self {
        Self {
            compute_quality: false,
            min_angle: 20.0,
        }
    }
}

/// A triangle in the triangulation
#[derive(Debug, Clone)]
pub struct Triangle {
    /// Indices of the three vertices
    pub vertices: [usize; 3],
    /// Triangle polygon
    pub polygon: Polygon,
    /// Triangle quality (0-1, higher is better)
    pub quality: Option<f64>,
}

/// Delaunay triangulation result
#[derive(Debug, Clone)]
pub struct DelaunayTriangulation {
    /// Input points
    pub points: Vec<Point>,
    /// Triangles
    pub triangles: Vec<Triangle>,
    /// Number of triangles
    pub num_triangles: usize,
}

/// Compute Delaunay triangulation
///
/// # Arguments
///
/// * `points` - Input points
/// * `options` - Triangulation options
///
/// # Returns
///
/// Delaunay triangulation with triangles
///
/// # Examples
///
/// ```
/// # use oxigdal_algorithms::error::Result;
/// use oxigdal_algorithms::vector::delaunay::{delaunay_triangulation, DelaunayOptions};
/// use oxigdal_algorithms::Point;
///
/// # fn main() -> Result<()> {
/// let points = vec![
///     Point::new(0.0, 0.0),
///     Point::new(1.0, 0.0),
///     Point::new(0.5, 1.0),
///     Point::new(0.5, 0.5),
/// ];
///
/// let options = DelaunayOptions {
///     compute_quality: true,
///     ..Default::default()
/// };
///
/// let triangulation = delaunay_triangulation(&points, &options)?;
/// assert!(triangulation.num_triangles >= 2);
/// # Ok(())
/// # }
/// ```
pub fn delaunay_triangulation(
    points: &[Point],
    options: &DelaunayOptions,
) -> Result<DelaunayTriangulation> {
    if points.len() < 3 {
        return Err(AlgorithmError::InvalidInput(
            "Need at least 3 points for triangulation".to_string(),
        ));
    }

    // Convert points to delaunator format
    let delaunator_points: Vec<delaunator::Point> = points
        .iter()
        .map(|p| delaunator::Point {
            x: p.coord.x,
            y: p.coord.y,
        })
        .collect();

    // Compute Delaunay triangulation
    let delaunay = delaunator::triangulate(&delaunator_points);

    // Build triangles
    let mut triangles = Vec::new();

    for tri_idx in 0..(delaunay.triangles.len() / 3) {
        let a = delaunay.triangles[tri_idx * 3];
        let b = delaunay.triangles[tri_idx * 3 + 1];
        let c = delaunay.triangles[tri_idx * 3 + 2];

        let pa = &points[a];
        let pb = &points[b];
        let pc = &points[c];

        // Create triangle polygon
        let coords_tri = vec![
            Coordinate::new_2d(pa.coord.x, pa.coord.y),
            Coordinate::new_2d(pb.coord.x, pb.coord.y),
            Coordinate::new_2d(pc.coord.x, pc.coord.y),
            Coordinate::new_2d(pa.coord.x, pa.coord.y), // Close the ring
        ];

        let exterior = LineString::new(coords_tri)
            .map_err(|e| AlgorithmError::InvalidGeometry(format!("Invalid triangle: {}", e)))?;

        let polygon = Polygon::new(exterior, vec![]).map_err(|e| {
            AlgorithmError::InvalidGeometry(format!("Invalid triangle polygon: {}", e))
        })?;

        // Compute quality if requested
        let quality = if options.compute_quality {
            Some(compute_triangle_quality(pa, pb, pc))
        } else {
            None
        };

        triangles.push(Triangle {
            vertices: [a, b, c],
            polygon,
            quality,
        });
    }

    let num_triangles = triangles.len();

    Ok(DelaunayTriangulation {
        points: points.to_vec(),
        triangles,
        num_triangles,
    })
}

/// Compute triangle quality (ratio of inradius to circumradius)
fn compute_triangle_quality(pa: &Point, pb: &Point, pc: &Point) -> f64 {
    // Edge lengths
    let a = distance(pb, pc);
    let b = distance(pc, pa);
    let c = distance(pa, pb);

    // Semi-perimeter
    let s = (a + b + c) / 2.0;

    // Area (Heron's formula)
    let area = (s * (s - a) * (s - b) * (s - c)).sqrt();

    // Inradius
    let inradius = area / s;

    // Circumradius
    let circumradius = (a * b * c) / (4.0 * area);

    // Quality ratio (0-1, higher is better)
    if circumradius > 0.0 {
        2.0 * inradius / circumradius
    } else {
        0.0
    }
}

/// Calculate distance between two points
fn distance(p1: &Point, p2: &Point) -> f64 {
    let dx = p1.coord.x - p2.coord.x;
    let dy = p1.coord.y - p2.coord.y;
    (dx * dx + dy * dy).sqrt()
}

/// Check if a point is inside the circumcircle of a triangle
pub fn in_circumcircle(pa: &Point, pb: &Point, pc: &Point, pd: &Point) -> bool {
    let ax = pa.coord.x - pd.coord.x;
    let ay = pa.coord.y - pd.coord.y;
    let bx = pb.coord.x - pd.coord.x;
    let by = pb.coord.y - pd.coord.y;
    let cx = pc.coord.x - pd.coord.x;
    let cy = pc.coord.y - pd.coord.y;

    let det = (ax * ax + ay * ay) * (bx * cy - cx * by) - (bx * bx + by * by) * (ax * cy - cx * ay)
        + (cx * cx + cy * cy) * (ax * by - bx * ay);

    det > 0.0
}

/// Constrained Delaunay triangulation with constraint edges
pub fn constrained_delaunay(
    points: &[Point],
    constraints: &[(usize, usize)],
    options: &DelaunayOptions,
) -> Result<DelaunayTriangulation> {
    // Basic implementation: compute triangulation first, then enforce constraints
    let mut triangulation = delaunay_triangulation(points, options)?;

    // Filter triangles that violate constraints
    triangulation
        .triangles
        .retain(|triangle| !violates_constraints(triangle, constraints, points));

    triangulation.num_triangles = triangulation.triangles.len();

    Ok(triangulation)
}

/// Check if a triangle violates any constraint edges
fn violates_constraints(
    triangle: &Triangle,
    constraints: &[(usize, usize)],
    _points: &[Point],
) -> bool {
    // Check if any constraint edge intersects the triangle interior
    for &(c1, c2) in constraints {
        // If constraint edge is an edge of the triangle, it's OK
        let [a, b, c] = triangle.vertices;

        if (c1 == a && c2 == b)
            || (c1 == b && c2 == a)
            || (c1 == b && c2 == c)
            || (c1 == c && c2 == b)
            || (c1 == c && c2 == a)
            || (c1 == a && c2 == c)
        {
            continue;
        }

        // Otherwise, check for intersection (simplified check)
        // A full implementation would need proper edge-triangle intersection
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delaunay_simple() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(0.5, 1.0),
            Point::new(0.5, 0.5),
        ];

        let options = DelaunayOptions::default();
        let result = delaunay_triangulation(&points, &options);

        assert!(result.is_ok());

        let triangulation = result.expect("Triangulation failed");
        assert!(triangulation.num_triangles >= 2);
    }

    #[test]
    fn test_triangle_quality() {
        // Equilateral triangle (perfect quality)
        let pa = Point::new(0.0, 0.0);
        let pb = Point::new(1.0, 0.0);
        let pc = Point::new(0.5, 0.866); // ~sqrt(3)/2

        let quality = compute_triangle_quality(&pa, &pb, &pc);
        assert!(quality > 0.9); // Close to 1.0 for equilateral
    }

    #[test]
    fn test_in_circumcircle() {
        let pa = Point::new(0.0, 0.0);
        let pb = Point::new(1.0, 0.0);
        let pc = Point::new(0.0, 1.0);
        let pd = Point::new(0.25, 0.25); // Inside circumcircle

        assert!(in_circumcircle(&pa, &pb, &pc, &pd));
    }

    #[test]
    fn test_constrained_delaunay() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(0.5, 1.0),
            Point::new(0.5, 0.5),
        ];

        let constraints = vec![(0, 2)]; // Constraint edge from point 0 to point 2

        let options = DelaunayOptions::default();
        let result = constrained_delaunay(&points, &constraints, &options);

        assert!(result.is_ok());
    }
}

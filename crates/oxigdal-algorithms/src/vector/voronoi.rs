//! Voronoi diagram generation
//!
//! Compute Voronoi diagrams (Thiessen polygons) from point sets.

use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::{Coordinate, LineString, Point, Polygon};

/// Options for Voronoi diagram generation
#[derive(Debug, Clone, Default)]
pub struct VoronoiOptions {
    /// Bounding box for clipping diagram
    pub bounds: Option<(f64, f64, f64, f64)>, // (min_x, min_y, max_x, max_y)
    /// Whether to include infinite cells
    pub include_infinite: bool,
}

/// A Voronoi cell (polygon)
#[derive(Debug, Clone)]
pub struct VoronoiCell {
    /// Site (generating point) for this cell
    pub site: Point,
    /// Site index
    pub site_index: usize,
    /// Polygon representing the cell
    pub polygon: Option<Polygon>,
    /// Whether this is an infinite cell
    pub is_infinite: bool,
}

/// Voronoi diagram result
#[derive(Debug, Clone)]
pub struct VoronoiDiagram {
    /// Voronoi cells
    pub cells: Vec<VoronoiCell>,
    /// Number of sites
    pub num_sites: usize,
}

/// Generate Voronoi diagram from points
///
/// # Arguments
///
/// * `points` - Input points (sites)
/// * `options` - Voronoi options
///
/// # Returns
///
/// Voronoi diagram with cells for each point
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::vector::voronoi::{voronoi_diagram, VoronoiOptions};
/// use oxigdal_algorithms::Point;
/// # use oxigdal_algorithms::error::Result;
///
/// # fn main() -> Result<()> {
/// let points = vec![
///     Point::new(0.0, 0.0),
///     Point::new(5.0, 0.0),
///     Point::new(2.5, 5.0),
/// ];
///
/// let options = VoronoiOptions {
///     bounds: Some((0.0, 0.0, 10.0, 10.0)),
///     include_infinite: false,
/// };
///
/// let diagram = voronoi_diagram(&points, &options)?;
/// assert_eq!(diagram.num_sites, 3);
/// # Ok(())
/// # }
/// ```
pub fn voronoi_diagram(points: &[Point], options: &VoronoiOptions) -> Result<VoronoiDiagram> {
    if points.len() < 3 {
        return Err(AlgorithmError::InvalidInput(
            "Need at least 3 points for Voronoi diagram".to_string(),
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

    // Build Voronoi cells from dual graph
    let mut cells = Vec::new();

    for (site_idx, point) in points.iter().enumerate() {
        let cell = build_voronoi_cell(site_idx, point, &delaunay, points, options)?;
        cells.push(cell);
    }

    Ok(VoronoiDiagram {
        cells,
        num_sites: points.len(),
    })
}

/// Build a Voronoi cell for a site
fn build_voronoi_cell(
    site_idx: usize,
    site: &Point,
    delaunay: &delaunator::Triangulation,
    points: &[Point],
    options: &VoronoiOptions,
) -> Result<VoronoiCell> {
    // Find all triangles containing this site
    let mut cell_vertices = Vec::new();
    let mut is_infinite = false;

    // Compute circumcenters of triangles
    for tri_idx in 0..(delaunay.triangles.len() / 3) {
        let a = delaunay.triangles[tri_idx * 3];
        let b = delaunay.triangles[tri_idx * 3 + 1];
        let c = delaunay.triangles[tri_idx * 3 + 2];

        if a == site_idx || b == site_idx || c == site_idx {
            // This triangle contains our site
            let pa = &points[a];
            let pb = &points[b];
            let pc = &points[c];

            let circumcenter = compute_circumcenter(
                pa.coord.x, pa.coord.y, pb.coord.x, pb.coord.y, pc.coord.x, pc.coord.y,
            )?;

            cell_vertices.push(circumcenter);
        }
    }

    // Check if cell is bounded
    if let Some((min_x, min_y, max_x, max_y)) = options.bounds {
        // Clip vertices to bounds
        cell_vertices.retain(|coord| {
            coord.x >= min_x && coord.x <= max_x && coord.y >= min_y && coord.y <= max_y
        });

        is_infinite = cell_vertices.len() < 3;
    }

    // Create polygon from vertices
    let polygon =
        if cell_vertices.len() >= 3 {
            // Sort vertices by angle around site
            cell_vertices.sort_by(|a, b| {
                let angle_a = (a.y - site.coord.y).atan2(a.x - site.coord.x);
                let angle_b = (b.y - site.coord.y).atan2(b.x - site.coord.x);
                angle_a
                    .partial_cmp(&angle_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            // Close the ring
            if let Some(first) = cell_vertices.first().copied() {
                cell_vertices.push(first);
            }

            // Create polygon
            let exterior = LineString::new(cell_vertices.clone()).map_err(|e| {
                AlgorithmError::InvalidGeometry(format!("Invalid cell exterior: {}", e))
            })?;
            Some(Polygon::new(exterior, vec![]).map_err(|e| {
                AlgorithmError::InvalidGeometry(format!("Invalid cell polygon: {}", e))
            })?)
        } else {
            None
        };

    Ok(VoronoiCell {
        site: site.clone(),
        site_index: site_idx,
        polygon,
        is_infinite,
    })
}

/// Compute circumcenter of a triangle
fn compute_circumcenter(
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    cx: f64,
    cy: f64,
) -> Result<Coordinate> {
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));

    if d.abs() < 1e-10 {
        return Err(AlgorithmError::ComputationError(
            "Degenerate triangle".to_string(),
        ));
    }

    let ux = ((ax * ax + ay * ay) * (by - cy)
        + (bx * bx + by * by) * (cy - ay)
        + (cx * cx + cy * cy) * (ay - by))
        / d;
    let uy = ((ax * ax + ay * ay) * (cx - bx)
        + (bx * bx + by * by) * (ax - cx)
        + (cx * cx + cy * cy) * (bx - ax))
        / d;

    Ok(Coordinate::new_2d(ux, uy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voronoi_simple() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(5.0, 0.0),
            Point::new(2.5, 5.0),
        ];

        let options = VoronoiOptions {
            bounds: Some((0.0, 0.0, 10.0, 10.0)),
            include_infinite: false,
        };

        let result = voronoi_diagram(&points, &options);
        assert!(result.is_ok());

        let diagram = result.expect("Voronoi failed");
        assert_eq!(diagram.num_sites, 3);
    }

    #[test]
    fn test_circumcenter() {
        let result = compute_circumcenter(0.0, 0.0, 1.0, 0.0, 0.0, 1.0);
        assert!(result.is_ok());

        let center = result.expect("Failed to compute circumcenter");
        assert!((center.x - 0.5).abs() < 1e-6);
        assert!((center.y - 0.5).abs() < 1e-6);
    }
}

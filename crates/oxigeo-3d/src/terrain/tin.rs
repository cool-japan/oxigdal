//! TIN (Triangulated Irregular Network) implementation
//!
//! Provides Delaunay triangulation for terrain modeling and surface analysis.

use crate::error::{Error, Result};
use crate::mesh::{Mesh, Vertex};
use crate::pointcloud::Point as CloudPoint;
use delaunator::{Point as DelaunayPoint, triangulate};
use serde::{Deserialize, Serialize};

/// TIN point (2D position with elevation)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TinPoint {
    /// X coordinate
    pub x: f64,
    /// Y coordinate
    pub y: f64,
    /// Z elevation
    pub z: f64,
}

impl TinPoint {
    /// Create a new TIN point
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Create from point cloud point
    pub fn from_cloud_point(point: &CloudPoint) -> Self {
        Self {
            x: point.x,
            y: point.y,
            z: point.z,
        }
    }

    /// Distance to another point (2D)
    pub fn distance_2d(&self, other: &TinPoint) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Distance to another point (3D)
    pub fn distance_3d(&self, other: &TinPoint) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

impl From<DelaunayPoint> for TinPoint {
    fn from(p: DelaunayPoint) -> Self {
        Self {
            x: p.x,
            y: p.y,
            z: 0.0,
        }
    }
}

/// TIN triangle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TinTriangle {
    /// Vertex indices
    pub vertices: [usize; 3],
}

impl TinTriangle {
    /// Create a new triangle
    pub fn new(v0: usize, v1: usize, v2: usize) -> Self {
        Self {
            vertices: [v0, v1, v2],
        }
    }

    /// Get vertices from TIN
    pub fn get_vertices<'a>(&self, tin: &'a Tin) -> [&'a TinPoint; 3] {
        [
            &tin.points[self.vertices[0]],
            &tin.points[self.vertices[1]],
            &tin.points[self.vertices[2]],
        ]
    }

    /// Calculate triangle area (2D)
    pub fn area_2d(&self, points: &[TinPoint]) -> f64 {
        let p0 = &points[self.vertices[0]];
        let p1 = &points[self.vertices[1]];
        let p2 = &points[self.vertices[2]];

        let x1 = p1.x - p0.x;
        let y1 = p1.y - p0.y;
        let x2 = p2.x - p0.x;
        let y2 = p2.y - p0.y;

        (x1 * y2 - x2 * y1).abs() / 2.0
    }

    /// Calculate triangle normal
    pub fn normal(&self, points: &[TinPoint]) -> [f64; 3] {
        let p0 = &points[self.vertices[0]];
        let p1 = &points[self.vertices[1]];
        let p2 = &points[self.vertices[2]];

        let v1 = [p1.x - p0.x, p1.y - p0.y, p1.z - p0.z];
        let v2 = [p2.x - p0.x, p2.y - p0.y, p2.z - p0.z];

        let nx = v1[1] * v2[2] - v1[2] * v2[1];
        let ny = v1[2] * v2[0] - v1[0] * v2[2];
        let nz = v1[0] * v2[1] - v1[1] * v2[0];

        let length = (nx * nx + ny * ny + nz * nz).sqrt();
        if length > 0.0 {
            [nx / length, ny / length, nz / length]
        } else {
            [0.0, 0.0, 1.0]
        }
    }

    /// Calculate triangle slope (in degrees)
    pub fn slope(&self, points: &[TinPoint]) -> f64 {
        let normal = self.normal(points);
        let angle = (normal[2].abs()).acos();
        angle.to_degrees()
    }

    /// Calculate triangle aspect (in degrees, 0 = North)
    pub fn aspect(&self, points: &[TinPoint]) -> f64 {
        let normal = self.normal(points);
        let aspect = normal[1].atan2(normal[0]).to_degrees();
        (90.0 - aspect + 360.0) % 360.0
    }
}

/// Triangulated Irregular Network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tin {
    /// Points
    pub points: Vec<TinPoint>,
    /// Triangles
    pub triangles: Vec<TinTriangle>,
}

impl Tin {
    /// Create a new empty TIN
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            triangles: Vec::new(),
        }
    }

    /// Create TIN with data
    pub fn with_data(points: Vec<TinPoint>, triangles: Vec<TinTriangle>) -> Self {
        Self { points, triangles }
    }

    /// Get number of points
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// Get number of triangles
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// Calculate elevation at a point using barycentric interpolation
    pub fn interpolate_elevation(&self, x: f64, y: f64) -> Option<f64> {
        // Find triangle containing the point
        for triangle in &self.triangles {
            let vertices = triangle.get_vertices(self);
            let p0 = vertices[0];
            let p1 = vertices[1];
            let p2 = vertices[2];

            // Barycentric coordinates
            let denom = (p1.y - p2.y) * (p0.x - p2.x) + (p2.x - p1.x) * (p0.y - p2.y);
            if denom.abs() < 1e-10 {
                continue;
            }

            let w0 = ((p1.y - p2.y) * (x - p2.x) + (p2.x - p1.x) * (y - p2.y)) / denom;
            let w1 = ((p2.y - p0.y) * (x - p2.x) + (p0.x - p2.x) * (y - p2.y)) / denom;
            let w2 = 1.0 - w0 - w1;

            // Check if point is inside triangle
            if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                return Some(w0 * p0.z + w1 * p1.z + w2 * p2.z);
            }
        }

        None
    }

    /// Calculate minimum elevation
    pub fn min_elevation(&self) -> Option<f64> {
        self.points
            .iter()
            .map(|p| p.z)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Calculate maximum elevation
    pub fn max_elevation(&self) -> Option<f64> {
        self.points
            .iter()
            .map(|p| p.z)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Generate contour lines at specified intervals
    pub fn generate_contours(&self, interval: f64) -> Vec<Vec<[f64; 2]>> {
        let mut contours = Vec::new();

        let min_z = self.min_elevation().unwrap_or(0.0);
        let max_z = self.max_elevation().unwrap_or(0.0);

        let mut z = (min_z / interval).ceil() * interval;

        while z <= max_z {
            let contour = self.extract_contour(z);
            if !contour.is_empty() {
                contours.push(contour);
            }
            z += interval;
        }

        contours
    }

    /// Extract contour line at specific elevation
    fn extract_contour(&self, elevation: f64) -> Vec<[f64; 2]> {
        let mut segments = Vec::new();

        for triangle in &self.triangles {
            let vertices = triangle.get_vertices(self);

            // Check each edge
            for i in 0..3 {
                let p0 = vertices[i];
                let p1 = vertices[(i + 1) % 3];

                // Check if edge crosses the elevation
                if (p0.z - elevation) * (p1.z - elevation) < 0.0 {
                    // Interpolate crossing point
                    let t = (elevation - p0.z) / (p1.z - p0.z);
                    let x = p0.x + t * (p1.x - p0.x);
                    let y = p0.y + t * (p1.y - p0.y);
                    segments.push([x, y]);
                }
            }
        }

        segments
    }

    /// Calculate slope for each triangle
    pub fn calculate_slopes(&self) -> Vec<f64> {
        self.triangles
            .iter()
            .map(|t| t.slope(&self.points))
            .collect()
    }

    /// Calculate aspect for each triangle
    pub fn calculate_aspects(&self) -> Vec<f64> {
        self.triangles
            .iter()
            .map(|t| t.aspect(&self.points))
            .collect()
    }

    /// Validate TIN structure
    pub fn validate(&self) -> Result<()> {
        let point_count = self.points.len();

        for (i, triangle) in self.triangles.iter().enumerate() {
            for &idx in &triangle.vertices {
                if idx >= point_count {
                    return Err(Error::Tin(format!(
                        "Triangle {} has invalid vertex index: {}",
                        i, idx
                    )));
                }
            }

            // Check for degenerate triangles
            if triangle.vertices[0] == triangle.vertices[1]
                || triangle.vertices[1] == triangle.vertices[2]
                || triangle.vertices[0] == triangle.vertices[2]
            {
                return Err(Error::Tin(format!(
                    "Triangle {} is degenerate: {:?}",
                    i, triangle.vertices
                )));
            }
        }

        Ok(())
    }
}

impl Default for Tin {
    fn default() -> Self {
        Self::new()
    }
}

/// Create TIN from point cloud points using Delaunay triangulation
pub fn create_tin(points: &[CloudPoint]) -> Result<Tin> {
    if points.is_empty() {
        return Err(Error::EmptyDataset("No points provided".to_string()));
    }

    if points.len() < 3 {
        return Err(Error::Tin(format!(
            "At least 3 points required for triangulation, got {}",
            points.len()
        )));
    }

    // Convert to TIN points and Delaunay points
    let tin_points: Vec<TinPoint> = points.iter().map(TinPoint::from_cloud_point).collect();

    let delaunay_points: Vec<DelaunayPoint> = tin_points
        .iter()
        .map(|p| DelaunayPoint { x: p.x, y: p.y })
        .collect();

    // Perform Delaunay triangulation
    let result = triangulate(&delaunay_points);

    // Convert triangulation result to TIN triangles
    let tin_triangles: Vec<TinTriangle> = result
        .triangles
        .chunks(3)
        .map(|chunk| TinTriangle::new(chunk[0], chunk[1], chunk[2]))
        .collect();

    let tin = Tin::with_data(tin_points, tin_triangles);
    tin.validate()?;

    Ok(tin)
}

/// Create TIN from raw points
pub fn create_tin_from_points(points: &[TinPoint]) -> Result<Tin> {
    if points.is_empty() {
        return Err(Error::EmptyDataset("No points provided".to_string()));
    }

    if points.len() < 3 {
        return Err(Error::Tin(format!(
            "At least 3 points required for triangulation, got {}",
            points.len()
        )));
    }

    let delaunay_points: Vec<DelaunayPoint> = points
        .iter()
        .map(|p| DelaunayPoint { x: p.x, y: p.y })
        .collect();

    let result = triangulate(&delaunay_points);

    let tin_triangles: Vec<TinTriangle> = result
        .triangles
        .chunks(3)
        .map(|chunk| TinTriangle::new(chunk[0], chunk[1], chunk[2]))
        .collect();

    let tin = Tin::with_data(points.to_vec(), tin_triangles);
    tin.validate()?;

    Ok(tin)
}

/// Convert TIN to 3D mesh
pub fn tin_to_mesh(tin: &Tin) -> Result<Mesh> {
    tin.validate()?;

    let mut mesh = Mesh::new();

    // Convert TIN points to mesh vertices
    for point in &tin.points {
        let vertex = Vertex::new([point.x as f32, point.y as f32, point.z as f32]);
        mesh.add_vertex(vertex);
    }

    // Convert TIN triangles to mesh triangles
    for triangle in &tin.triangles {
        mesh.add_triangle(
            triangle.vertices[0] as u32,
            triangle.vertices[1] as u32,
            triangle.vertices[2] as u32,
        );
    }

    // Calculate normals
    mesh.calculate_normals();

    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_tin_point_distance() {
        let p1 = TinPoint::new(0.0, 0.0, 0.0);
        let p2 = TinPoint::new(3.0, 4.0, 0.0);

        assert_relative_eq!(p1.distance_2d(&p2), 5.0);
    }

    #[test]
    fn test_create_tin_simple() {
        let points = vec![
            CloudPoint::new(0.0, 0.0, 0.0),
            CloudPoint::new(1.0, 0.0, 1.0),
            CloudPoint::new(0.0, 1.0, 1.0),
            CloudPoint::new(1.0, 1.0, 2.0),
        ];

        let tin = create_tin(&points);
        assert!(tin.is_ok());

        let tin = tin.expect("Failed to create TIN from valid points");
        assert_eq!(tin.point_count(), 4);
        assert!(tin.triangle_count() > 0);
    }

    #[test]
    fn test_create_tin_insufficient_points() {
        let points = vec![
            CloudPoint::new(0.0, 0.0, 0.0),
            CloudPoint::new(1.0, 0.0, 0.0),
        ];

        let tin = create_tin(&points);
        assert!(tin.is_err());
    }

    #[test]
    fn test_tin_interpolation() {
        let points = vec![
            TinPoint::new(0.0, 0.0, 0.0),
            TinPoint::new(1.0, 0.0, 0.0),
            TinPoint::new(0.0, 1.0, 0.0),
            TinPoint::new(1.0, 1.0, 1.0),
        ];

        let tin = create_tin_from_points(&points)
            .expect("Failed to create TIN from test points for interpolation test");

        // Test interpolation at center
        let z = tin.interpolate_elevation(0.5, 0.5);
        assert!(z.is_some());
    }

    #[test]
    fn test_tin_min_max_elevation() {
        let points = vec![
            TinPoint::new(0.0, 0.0, 10.0),
            TinPoint::new(1.0, 0.0, 20.0),
            TinPoint::new(0.0, 1.0, 5.0),
        ];

        let tin = create_tin_from_points(&points)
            .expect("Failed to create TIN from test points for elevation test");

        assert_relative_eq!(
            tin.min_elevation().expect("Failed to get min elevation"),
            5.0
        );
        assert_relative_eq!(
            tin.max_elevation().expect("Failed to get max elevation"),
            20.0
        );
    }

    #[test]
    fn test_tin_to_mesh() {
        let points = vec![
            CloudPoint::new(0.0, 0.0, 0.0),
            CloudPoint::new(1.0, 0.0, 0.0),
            CloudPoint::new(0.0, 1.0, 0.0),
        ];

        let tin = create_tin(&points).expect("Failed to create TIN from test points for mesh test");
        let mesh = tin_to_mesh(&tin);

        assert!(mesh.is_ok());
        let mesh = mesh.expect("Failed to convert TIN to mesh");
        assert_eq!(mesh.vertex_count(), 3);
        assert!(mesh.triangle_count() > 0);
    }

    #[test]
    fn test_triangle_slope() {
        let points = vec![
            TinPoint::new(0.0, 0.0, 0.0),
            TinPoint::new(1.0, 0.0, 0.0),
            TinPoint::new(0.0, 1.0, 1.0), // 45 degree slope
        ];

        let triangle = TinTriangle::new(0, 1, 2);
        let slope = triangle.slope(&points);

        // Should be approximately 45 degrees
        assert!(slope > 30.0 && slope < 60.0);
    }
}

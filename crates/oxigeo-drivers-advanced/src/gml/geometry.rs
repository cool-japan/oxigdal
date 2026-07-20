//! GML geometry types.

use serde::{Deserialize, Serialize};

/// GML geometry enumeration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GmlGeometry {
    /// Point geometry
    Point {
        /// Coordinates [x, y] or [x, y, z]
        coordinates: Vec<f64>,
    },
    /// LineString geometry
    LineString {
        /// Sequence of coordinate points
        coordinates: Vec<Vec<f64>>,
    },
    /// Polygon geometry
    Polygon {
        /// Exterior ring coordinates
        exterior: Vec<Vec<f64>>,
        /// Interior ring coordinates (holes)
        interior: Vec<Vec<Vec<f64>>>,
    },
    /// Multi-geometry collection
    MultiGeometry {
        /// Collection of geometries
        geometries: Vec<GmlGeometry>,
    },
}

/// GML Point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmlPoint {
    /// Coordinates [x, y] or [x, y, z]
    pub coordinates: Vec<f64>,
}

impl GmlPoint {
    /// Create new point.
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            coordinates: vec![x, y],
        }
    }

    /// Create with Z coordinate.
    pub fn with_z(x: f64, y: f64, z: f64) -> Self {
        Self {
            coordinates: vec![x, y, z],
        }
    }

    /// Get X coordinate.
    pub fn x(&self) -> Option<f64> {
        self.coordinates.first().copied()
    }

    /// Get Y coordinate.
    pub fn y(&self) -> Option<f64> {
        self.coordinates.get(1).copied()
    }

    /// Get Z coordinate.
    pub fn z(&self) -> Option<f64> {
        self.coordinates.get(2).copied()
    }
}

/// GML LineString.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmlLineString {
    /// Coordinates
    pub coordinates: Vec<Vec<f64>>,
}

impl GmlLineString {
    /// Create new linestring.
    pub fn new(coordinates: Vec<Vec<f64>>) -> Self {
        Self { coordinates }
    }

    /// Get point count.
    pub fn len(&self) -> usize {
        self.coordinates.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.coordinates.is_empty()
    }
}

/// GML Polygon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmlPolygon {
    /// Exterior ring
    pub exterior: Vec<Vec<f64>>,
    /// Interior rings (holes)
    pub interior: Vec<Vec<Vec<f64>>>,
}

impl GmlPolygon {
    /// Create new polygon.
    pub fn new(exterior: Vec<Vec<f64>>) -> Self {
        Self {
            exterior,
            interior: Vec::new(),
        }
    }

    /// Add interior ring.
    pub fn add_interior(&mut self, ring: Vec<Vec<f64>>) {
        self.interior.push(ring);
    }

    /// Check if has holes.
    pub fn has_holes(&self) -> bool {
        !self.interior.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gml_point() {
        let pt = GmlPoint::new(10.0, 20.0);
        assert_eq!(pt.x(), Some(10.0));
        assert_eq!(pt.y(), Some(20.0));
        assert_eq!(pt.z(), None);

        let pt_3d = GmlPoint::with_z(10.0, 20.0, 30.0);
        assert_eq!(pt_3d.z(), Some(30.0));
    }

    #[test]
    fn test_gml_linestring() {
        let coords = vec![vec![0.0, 0.0], vec![10.0, 10.0]];
        let line = GmlLineString::new(coords);
        assert_eq!(line.len(), 2);
        assert!(!line.is_empty());
    }

    #[test]
    fn test_gml_polygon() {
        let exterior = vec![
            vec![0.0, 0.0],
            vec![10.0, 0.0],
            vec![10.0, 10.0],
            vec![0.0, 10.0],
            vec![0.0, 0.0],
        ];
        let mut poly = GmlPolygon::new(exterior);
        assert!(!poly.has_holes());

        let interior = vec![
            vec![2.0, 2.0],
            vec![8.0, 2.0],
            vec![8.0, 8.0],
            vec![2.0, 8.0],
            vec![2.0, 2.0],
        ];
        poly.add_interior(interior);
        assert!(poly.has_holes());
    }
}

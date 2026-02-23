//! ClickHouse spatial data types.

use serde::{Deserialize, Serialize};

/// Point type for ClickHouse (Tuple(Float64, Float64)).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, clickhouse::Row)]
pub struct Point {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
}

impl Point {
    /// Create a new point.
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Convert to tuple.
    pub fn to_tuple(self) -> (f64, f64) {
        (self.x, self.y)
    }

    /// Create from tuple.
    pub fn from_tuple(tuple: (f64, f64)) -> Self {
        Self {
            x: tuple.0,
            y: tuple.1,
        }
    }
}

impl From<geo_types::Point<f64>> for Point {
    fn from(p: geo_types::Point<f64>) -> Self {
        Self { x: p.x(), y: p.y() }
    }
}

impl From<Point> for geo_types::Point<f64> {
    fn from(p: Point) -> Self {
        geo_types::Point::new(p.x, p.y)
    }
}

/// Ring type for ClickHouse (Array of Points).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ring {
    /// Points forming the ring.
    pub points: Vec<Point>,
}

impl Ring {
    /// Create a new ring.
    pub fn new(points: Vec<Point>) -> Self {
        Self { points }
    }

    /// Check if ring is closed.
    pub fn is_closed(&self) -> bool {
        if self.points.len() < 2 {
            return false;
        }
        let first = &self.points[0];
        let last = &self.points[self.points.len() - 1];
        (first.x - last.x).abs() < f64::EPSILON && (first.y - last.y).abs() < f64::EPSILON
    }
}

/// Polygon type for ClickHouse (Array of Rings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Polygon {
    /// Exterior ring of the polygon.
    pub exterior: Ring,
    /// Interior rings (holes) of the polygon.
    pub interiors: Vec<Ring>,
}

impl Polygon {
    /// Create a new polygon.
    pub fn new(exterior: Ring) -> Self {
        Self {
            exterior,
            interiors: Vec::new(),
        }
    }

    /// Add an interior ring (hole).
    pub fn add_interior(&mut self, interior: Ring) {
        self.interiors.push(interior);
    }
}

impl From<geo_types::Polygon<f64>> for Polygon {
    fn from(poly: geo_types::Polygon<f64>) -> Self {
        let exterior_points: Vec<Point> = poly
            .exterior()
            .coords()
            .map(|c| Point::new(c.x, c.y))
            .collect();

        let interiors: Vec<Ring> = poly
            .interiors()
            .iter()
            .map(|ring| {
                let points: Vec<Point> = ring.coords().map(|c| Point::new(c.x, c.y)).collect();
                Ring::new(points)
            })
            .collect();

        Self {
            exterior: Ring::new(exterior_points),
            interiors,
        }
    }
}

/// MultiPolygon type for ClickHouse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiPolygon {
    /// Collection of polygons.
    pub polygons: Vec<Polygon>,
}

impl MultiPolygon {
    /// Create a new multipolygon.
    pub fn new(polygons: Vec<Polygon>) -> Self {
        Self { polygons }
    }
}

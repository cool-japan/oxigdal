//! Cassandra spatial data types.

use serde::{Deserialize, Serialize};

/// Point UDT for Cassandra.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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

/// Bounding box type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BBox {
    /// Minimum X coordinate.
    pub min_x: f64,
    /// Minimum Y coordinate.
    pub min_y: f64,
    /// Maximum X coordinate.
    pub max_x: f64,
    /// Maximum Y coordinate.
    pub max_y: f64,
}

impl BBox {
    /// Create a new bounding box.
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Check if a point is within the bounding box.
    pub fn contains(&self, point: &Point) -> bool {
        point.x >= self.min_x
            && point.x <= self.max_x
            && point.y >= self.min_y
            && point.y <= self.max_y
    }
}

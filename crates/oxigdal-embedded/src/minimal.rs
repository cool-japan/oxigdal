//! Minimal feature set for ultra-constrained environments
//!
//! Provides lightweight geospatial primitives with minimal memory footprint

use crate::error::{EmbeddedError, Result};
use core::fmt;

/// Minimal coordinate representation (32-bit floats)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimalCoordinate {
    /// Longitude or X coordinate
    pub x: f32,
    /// Latitude or Y coordinate
    pub y: f32,
}

impl MinimalCoordinate {
    /// Create a new coordinate
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Calculate distance to another coordinate (Euclidean)
    pub fn distance_to(&self, other: &Self) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        libm::sqrtf(dx * dx + dy * dy)
    }

    /// Check if coordinate is valid (not NaN or infinite)
    pub fn is_valid(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl fmt::Display for MinimalCoordinate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// Minimal bounding box representation
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimalBounds {
    /// Minimum X
    pub min_x: f32,
    /// Minimum Y
    pub min_y: f32,
    /// Maximum X
    pub max_x: f32,
    /// Maximum Y
    pub max_y: f32,
}

impl MinimalBounds {
    /// Create a new bounding box
    pub const fn new(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Create from center and size
    pub fn from_center(center: MinimalCoordinate, width: f32, height: f32) -> Self {
        let half_w = width / 2.0;
        let half_h = height / 2.0;

        Self {
            min_x: center.x - half_w,
            min_y: center.y - half_h,
            max_x: center.x + half_w,
            max_y: center.y + half_h,
        }
    }

    /// Check if point is inside bounds
    pub fn contains(&self, point: &MinimalCoordinate) -> bool {
        point.x >= self.min_x
            && point.x <= self.max_x
            && point.y >= self.min_y
            && point.y <= self.max_y
    }

    /// Check if bounds intersect
    pub fn intersects(&self, other: &Self) -> bool {
        !(self.max_x < other.min_x
            || self.min_x > other.max_x
            || self.max_y < other.min_y
            || self.min_y > other.max_y)
    }

    /// Get width
    pub fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    /// Get height
    pub fn height(&self) -> f32 {
        self.max_y - self.min_y
    }

    /// Get area
    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }

    /// Get center point
    pub fn center(&self) -> MinimalCoordinate {
        MinimalCoordinate::new(
            (self.min_x + self.max_x) / 2.0,
            (self.min_y + self.max_y) / 2.0,
        )
    }

    /// Expand bounds to include point
    pub fn expand_to_include(&mut self, point: &MinimalCoordinate) {
        if point.x < self.min_x {
            self.min_x = point.x;
        }
        if point.x > self.max_x {
            self.max_x = point.x;
        }
        if point.y < self.min_y {
            self.min_y = point.y;
        }
        if point.y > self.max_y {
            self.max_y = point.y;
        }
    }

    /// Check if bounds are valid
    pub fn is_valid(&self) -> bool {
        self.min_x <= self.max_x
            && self.min_y <= self.max_y
            && self.min_x.is_finite()
            && self.max_x.is_finite()
            && self.min_y.is_finite()
            && self.max_y.is_finite()
    }
}

/// Minimal raster metadata
#[derive(Debug, Clone, Copy)]
pub struct MinimalRasterMeta {
    /// Width in pixels
    pub width: u16,
    /// Height in pixels
    pub height: u16,
    /// Number of bands
    pub bands: u8,
    /// Data type size in bytes
    pub pixel_size: u8,
}

impl MinimalRasterMeta {
    /// Create new raster metadata
    pub const fn new(width: u16, height: u16, bands: u8, pixel_size: u8) -> Self {
        Self {
            width,
            height,
            bands,
            pixel_size,
        }
    }

    /// Calculate total size in bytes
    pub const fn total_size(&self) -> usize {
        self.width as usize * self.height as usize * self.bands as usize * self.pixel_size as usize
    }

    /// Calculate band size in bytes
    pub const fn band_size(&self) -> usize {
        self.width as usize * self.height as usize * self.pixel_size as usize
    }

    /// Calculate row size in bytes
    pub const fn row_size(&self) -> usize {
        self.width as usize * self.pixel_size as usize
    }
}

/// Minimal transformation (affine transform coefficients)
#[derive(Debug, Clone, Copy)]
pub struct MinimalTransform {
    /// Top-left X coordinate
    pub x0: f32,
    /// Pixel width
    pub dx: f32,
    /// Rotation (usually 0)
    pub rx: f32,
    /// Top-left Y coordinate
    pub y0: f32,
    /// Rotation (usually 0)
    pub ry: f32,
    /// Pixel height (usually negative)
    pub dy: f32,
}

impl MinimalTransform {
    /// Create identity transform
    pub const fn identity() -> Self {
        Self {
            x0: 0.0,
            dx: 1.0,
            rx: 0.0,
            y0: 0.0,
            ry: 0.0,
            dy: -1.0,
        }
    }

    /// Create simple transform (no rotation)
    pub const fn new_simple(x0: f32, y0: f32, pixel_width: f32, pixel_height: f32) -> Self {
        Self {
            x0,
            dx: pixel_width,
            rx: 0.0,
            y0,
            ry: 0.0,
            dy: pixel_height,
        }
    }

    /// Transform pixel coordinates to world coordinates
    pub fn pixel_to_world(&self, col: u16, row: u16) -> MinimalCoordinate {
        let x = self.x0 + self.dx * col as f32 + self.rx * row as f32;
        let y = self.y0 + self.ry * col as f32 + self.dy * row as f32;
        MinimalCoordinate::new(x, y)
    }

    /// Transform world coordinates to pixel coordinates
    pub fn world_to_pixel(&self, coord: &MinimalCoordinate) -> Result<(u16, u16)> {
        // Inverse transform (simplified for non-rotated case)
        if self.rx != 0.0 || self.ry != 0.0 {
            return Err(EmbeddedError::UnsupportedOperation);
        }

        let col = ((coord.x - self.x0) / self.dx) as i32;
        let row = ((coord.y - self.y0) / self.dy) as i32;

        if col < 0 || row < 0 || col > u16::MAX as i32 || row > u16::MAX as i32 {
            return Err(EmbeddedError::OutOfBounds {
                index: col.max(row) as usize,
                max: u16::MAX as usize,
            });
        }

        Ok((col as u16, row as u16))
    }
}

/// Minimal vector feature (point, line, or polygon)
#[derive(Debug, Clone)]
pub struct MinimalFeature<const MAX_POINTS: usize> {
    /// Feature points
    pub points: heapless::Vec<MinimalCoordinate, MAX_POINTS>,
    /// Feature type
    pub feature_type: FeatureType,
}

/// Feature type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureType {
    /// Point feature
    Point,
    /// Line feature
    Line,
    /// Polygon feature (closed line)
    Polygon,
}

impl<const MAX_POINTS: usize> MinimalFeature<MAX_POINTS> {
    /// Create a new feature
    pub const fn new(feature_type: FeatureType) -> Self {
        Self {
            points: heapless::Vec::new(),
            feature_type,
        }
    }

    /// Add a point to the feature
    pub fn add_point(&mut self, point: MinimalCoordinate) -> Result<()> {
        self.points
            .push(point)
            .map_err(|_| EmbeddedError::BufferTooSmall {
                required: 1,
                available: 0,
            })
    }

    /// Get bounding box
    pub fn bounds(&self) -> Option<MinimalBounds> {
        if self.points.is_empty() {
            return None;
        }

        let first = self.points[0];
        let mut bounds = MinimalBounds::new(first.x, first.y, first.x, first.y);

        for point in &self.points {
            bounds.expand_to_include(point);
        }

        Some(bounds)
    }

    /// Calculate total length (for lines and polygons)
    pub fn length(&self) -> f32 {
        if self.points.len() < 2 {
            return 0.0;
        }

        let mut total = 0.0;
        for i in 0..(self.points.len() - 1) {
            total += self.points[i].distance_to(&self.points[i + 1]);
        }

        // Close polygon
        if self.feature_type == FeatureType::Polygon && self.points.len() > 2 {
            if let (Some(first), Some(last)) = (self.points.first(), self.points.last()) {
                total += last.distance_to(first);
            }
        }

        total
    }

    /// Calculate area (for polygons only, using shoelace formula)
    pub fn area(&self) -> Result<f32> {
        if self.feature_type != FeatureType::Polygon {
            return Err(EmbeddedError::UnsupportedOperation);
        }

        if self.points.len() < 3 {
            return Ok(0.0);
        }

        let mut area = 0.0;
        let n = self.points.len();

        for i in 0..n {
            let j = (i + 1) % n;
            area += self.points[i].x * self.points[j].y;
            area -= self.points[j].x * self.points[i].y;
        }

        Ok(libm::fabsf(area) / 2.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinate() {
        let coord = MinimalCoordinate::new(10.0, 20.0);
        assert_eq!(coord.x, 10.0);
        assert_eq!(coord.y, 20.0);
        assert!(coord.is_valid());

        let other = MinimalCoordinate::new(13.0, 24.0);
        let dist = coord.distance_to(&other);
        assert!((dist - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_bounds() {
        let bounds = MinimalBounds::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(bounds.width(), 10.0);
        assert_eq!(bounds.height(), 10.0);
        assert_eq!(bounds.area(), 100.0);

        let point = MinimalCoordinate::new(5.0, 5.0);
        assert!(bounds.contains(&point));

        let outside = MinimalCoordinate::new(15.0, 15.0);
        assert!(!bounds.contains(&outside));
    }

    #[test]
    fn test_bounds_intersection() {
        let b1 = MinimalBounds::new(0.0, 0.0, 10.0, 10.0);
        let b2 = MinimalBounds::new(5.0, 5.0, 15.0, 15.0);
        let b3 = MinimalBounds::new(20.0, 20.0, 30.0, 30.0);

        assert!(b1.intersects(&b2));
        assert!(!b1.intersects(&b3));
    }

    #[test]
    fn test_raster_meta() {
        let meta = MinimalRasterMeta::new(100, 100, 3, 1);
        assert_eq!(meta.total_size(), 30000);
        assert_eq!(meta.band_size(), 10000);
        assert_eq!(meta.row_size(), 100);
    }

    #[test]
    fn test_transform() {
        let transform = MinimalTransform::new_simple(0.0, 100.0, 1.0, -1.0);
        let coord = transform.pixel_to_world(10, 10);
        assert_eq!(coord.x, 10.0);
        assert_eq!(coord.y, 90.0);

        let (col, row) = transform.world_to_pixel(&coord).expect("transform failed");
        assert_eq!(col, 10);
        assert_eq!(row, 10);
    }

    #[test]
    fn test_feature_line() {
        let mut line = MinimalFeature::<16>::new(FeatureType::Line);
        line.add_point(MinimalCoordinate::new(0.0, 0.0))
            .expect("add failed");
        line.add_point(MinimalCoordinate::new(3.0, 4.0))
            .expect("add failed");

        let length = line.length();
        assert!((length - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_feature_polygon() {
        let mut poly = MinimalFeature::<16>::new(FeatureType::Polygon);
        poly.add_point(MinimalCoordinate::new(0.0, 0.0))
            .expect("add failed");
        poly.add_point(MinimalCoordinate::new(10.0, 0.0))
            .expect("add failed");
        poly.add_point(MinimalCoordinate::new(10.0, 10.0))
            .expect("add failed");
        poly.add_point(MinimalCoordinate::new(0.0, 10.0))
            .expect("add failed");

        let area = poly.area().expect("area calculation failed");
        assert!((area - 100.0).abs() < 0.001);
    }
}

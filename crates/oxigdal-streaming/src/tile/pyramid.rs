//! Tile pyramid and matrix implementations.

use super::protocol::TileCoordinate;
use crate::error::{Result, StreamingError};
use oxigdal_core::types::BoundingBox;
use serde::{Deserialize, Serialize};

/// Zoom level information.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ZoomLevel {
    /// Zoom level index
    pub level: u8,

    /// Number of tiles in X direction
    pub matrix_width: u32,

    /// Number of tiles in Y direction
    pub matrix_height: u32,

    /// Resolution in units per pixel
    pub resolution: f64,

    /// Scale denominator
    pub scale_denominator: f64,
}

impl ZoomLevel {
    /// Create a new zoom level.
    pub fn new(level: u8, resolution: f64) -> Self {
        let tiles = 1u32 << level;
        Self {
            level,
            matrix_width: tiles,
            matrix_height: tiles,
            resolution,
            scale_denominator: resolution * 111_319.49079327358, // meters per degree
        }
    }

    /// Get the number of tiles at this zoom level.
    pub fn num_tiles(&self) -> u64 {
        (self.matrix_width as u64) * (self.matrix_height as u64)
    }
}

/// Tile matrix definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileMatrix {
    /// Matrix identifier
    pub identifier: String,

    /// Bounding box
    pub bbox: BoundingBox,

    /// Tile width in pixels
    pub tile_width: u32,

    /// Tile height in pixels
    pub tile_height: u32,

    /// Zoom levels
    pub zoom_levels: Vec<ZoomLevel>,
}

impl TileMatrix {
    /// Create a new tile matrix.
    pub fn new(
        identifier: String,
        bbox: BoundingBox,
        tile_width: u32,
        tile_height: u32,
    ) -> Self {
        Self {
            identifier,
            bbox,
            tile_width,
            tile_height,
            zoom_levels: Vec::new(),
        }
    }

    /// Add a zoom level.
    pub fn add_zoom_level(&mut self, level: ZoomLevel) {
        self.zoom_levels.push(level);
    }

    /// Get a zoom level by index.
    pub fn get_zoom_level(&self, level: u8) -> Option<&ZoomLevel> {
        self.zoom_levels.iter().find(|z| z.level == level)
    }

    /// Get the bounding box for a specific tile.
    pub fn tile_bbox(&self, coord: &TileCoordinate) -> Result<BoundingBox> {
        let zoom = self.get_zoom_level(coord.z)
            .ok_or_else(|| StreamingError::InvalidOperation(
                format!("Zoom level {} not found", coord.z)
            ))?;

        let width = self.bbox.width() / (zoom.matrix_width as f64);
        let height = self.bbox.height() / (zoom.matrix_height as f64);

        let min_x = self.bbox.min_x + (coord.x as f64) * width;
        let max_y = self.bbox.max_y - (coord.y as f64) * height;
        let max_x = min_x + width;
        let min_y = max_y - height;

        BoundingBox::new(min_x, min_y, max_x, max_y)
            .map_err(|e| StreamingError::Core(e))
    }
}

/// Tile pyramid for multi-resolution data.
pub struct TilePyramid {
    /// Tile matrices
    matrices: Vec<TileMatrix>,

    /// Minimum zoom level
    min_zoom: u8,

    /// Maximum zoom level
    max_zoom: u8,
}

impl TilePyramid {
    /// Create a new tile pyramid.
    pub fn new(min_zoom: u8, max_zoom: u8) -> Self {
        Self {
            matrices: Vec::new(),
            min_zoom,
            max_zoom,
        }
    }

    /// Create a standard Web Mercator pyramid.
    pub fn web_mercator(max_zoom: u8) -> Result<Self> {
        let bbox = BoundingBox::new(-180.0, -85.0511, 180.0, 85.0511)
            .map_err(|e| StreamingError::Core(e))?;

        let mut pyramid = Self::new(0, max_zoom);

        for z in 0..=max_zoom {
            let mut matrix = TileMatrix::new(
                format!("WebMercator:{}", z),
                bbox.clone(),
                256,
                256,
            );

            let resolution = 360.0 / (256.0 * (1u32 << z) as f64);
            matrix.add_zoom_level(ZoomLevel::new(z, resolution));

            pyramid.add_matrix(matrix);
        }

        Ok(pyramid)
    }

    /// Add a tile matrix.
    pub fn add_matrix(&mut self, matrix: TileMatrix) {
        self.matrices.push(matrix);
    }

    /// Get a tile matrix by identifier.
    pub fn get_matrix(&self, identifier: &str) -> Option<&TileMatrix> {
        self.matrices.iter().find(|m| m.identifier == identifier)
    }

    /// Get all tile coordinates for a zoom level.
    pub fn tiles_for_zoom(&self, zoom: u8) -> Vec<TileCoordinate> {
        if zoom < self.min_zoom || zoom > self.max_zoom {
            return Vec::new();
        }

        let num_tiles = 1u32 << zoom;
        let mut tiles = Vec::with_capacity((num_tiles * num_tiles) as usize);

        for y in 0..num_tiles {
            for x in 0..num_tiles {
                tiles.push(TileCoordinate::new(zoom, x, y));
            }
        }

        tiles
    }

    /// Get all tile coordinates that intersect a bounding box.
    pub fn tiles_for_bbox(&self, bbox: &BoundingBox, zoom: u8) -> Result<Vec<TileCoordinate>> {
        if zoom < self.min_zoom || zoom > self.max_zoom {
            return Ok(Vec::new());
        }

        let matrix = self.matrices.get(zoom as usize)
            .ok_or_else(|| StreamingError::InvalidOperation(
                format!("Matrix for zoom {} not found", zoom)
            ))?;

        let zoom_level = matrix.get_zoom_level(zoom)
            .ok_or_else(|| StreamingError::InvalidOperation(
                format!("Zoom level {} not found", zoom)
            ))?;

        let width = matrix.bbox.width() / (zoom_level.matrix_width as f64);
        let height = matrix.bbox.height() / (zoom_level.matrix_height as f64);

        let min_x = ((bbox.min_x - matrix.bbox.min_x) / width).floor().max(0.0) as u32;
        let max_x = ((bbox.max_x - matrix.bbox.min_x) / width).ceil().min(zoom_level.matrix_width as f64) as u32;
        let min_y = ((matrix.bbox.max_y - bbox.max_y) / height).floor().max(0.0) as u32;
        let max_y = ((matrix.bbox.max_y - bbox.min_y) / height).ceil().min(zoom_level.matrix_height as f64) as u32;

        let mut tiles = Vec::new();
        for y in min_y..max_y {
            for x in min_x..max_x {
                tiles.push(TileCoordinate::new(zoom, x, y));
            }
        }

        Ok(tiles)
    }

    /// Get the zoom levels.
    pub fn zoom_levels(&self) -> (u8, u8) {
        (self.min_zoom, self.max_zoom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zoom_level() {
        let zoom = ZoomLevel::new(10, 0.0001);
        assert_eq!(zoom.level, 10);
        assert_eq!(zoom.matrix_width, 1024);
        assert_eq!(zoom.matrix_height, 1024);
        assert_eq!(zoom.num_tiles(), 1024 * 1024);
    }

    #[test]
    fn test_web_mercator_pyramid() {
        let pyramid = TilePyramid::web_mercator(18);
        assert!(pyramid.is_ok());

        if let Ok(pyramid) = pyramid {
            assert_eq!(pyramid.zoom_levels(), (0, 18));
            assert_eq!(pyramid.matrices.len(), 19);
        }
    }

    #[test]
    fn test_tiles_for_zoom() {
        let pyramid = TilePyramid::new(0, 10);
        let tiles = pyramid.tiles_for_zoom(2);
        assert_eq!(tiles.len(), 16); // 4x4 = 16 tiles at zoom 2
    }
}

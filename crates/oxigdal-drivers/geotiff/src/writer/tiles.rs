//! Tile processing for GeoTIFF writing
//!
//! This module handles tiling of raster data and compression.

use oxigdal_core::error::Result;

use crate::compression;
use crate::tiff::{Compression, Predictor};

/// Information about a tile
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields will be used in future COG writer enhancements
pub struct TileInfo {
    /// Tile index in X direction
    pub tile_x: u32,
    /// Tile index in Y direction
    pub tile_y: u32,
    /// Compressed tile data
    pub data: Vec<u8>,
    /// Uncompressed size
    pub uncompressed_size: usize,
}

/// Tile processor for converting raster data into tiles
pub struct TileProcessor {
    /// Tile width
    tile_width: u32,
    /// Tile height
    tile_height: u32,
    /// Image width
    image_width: u64,
    /// Image height
    image_height: u64,
    /// Bytes per sample
    bytes_per_sample: usize,
    /// Samples per pixel (band count)
    samples_per_pixel: usize,
    /// Compression scheme
    compression: Compression,
    /// Predictor
    predictor: Predictor,
}

impl TileProcessor {
    /// Creates a new tile processor
    #[must_use]
    pub const fn new(
        tile_width: u32,
        tile_height: u32,
        image_width: u64,
        image_height: u64,
        bytes_per_sample: usize,
        samples_per_pixel: usize,
        compression: Compression,
        predictor: Predictor,
    ) -> Self {
        Self {
            tile_width,
            tile_height,
            image_height,
            image_width,
            bytes_per_sample,
            samples_per_pixel,
            compression,
            predictor,
        }
    }

    /// Returns the number of tiles across
    #[must_use]
    pub fn tiles_across(&self) -> u32 {
        self.image_width.div_ceil(u64::from(self.tile_width)) as u32
    }

    /// Returns the number of tiles down
    #[must_use]
    pub fn tiles_down(&self) -> u32 {
        self.image_height.div_ceil(u64::from(self.tile_height)) as u32
    }

    /// Returns the total number of tiles
    #[must_use]
    pub fn tile_count(&self) -> u32 {
        self.tiles_across() * self.tiles_down()
    }

    /// Extracts and compresses a tile from raster data
    ///
    /// # Arguments
    /// * `data` - Full raster data (row-major, interleaved)
    /// * `tile_x` - Tile index in X direction
    /// * `tile_y` - Tile index in Y direction
    ///
    /// # Errors
    /// Returns an error if compression fails
    pub fn process_tile(&self, data: &[u8], tile_x: u32, tile_y: u32) -> Result<TileInfo> {
        // Calculate tile bounds
        let x_start = u64::from(tile_x * self.tile_width);
        let y_start = u64::from(tile_y * self.tile_height);
        let x_end = (x_start + u64::from(self.tile_width)).min(self.image_width);
        let y_end = (y_start + u64::from(self.tile_height)).min(self.image_height);

        let tile_actual_width = (x_end - x_start) as u32;
        let tile_actual_height = (y_end - y_start) as u32;

        // Extract tile data
        let mut tile_data = self.extract_tile(
            data,
            x_start,
            y_start,
            tile_actual_width,
            tile_actual_height,
        );

        let uncompressed_size = tile_data.len();

        // Apply predictor
        compression::apply_predictor_forward(
            &mut tile_data,
            self.predictor,
            self.bytes_per_sample,
            self.samples_per_pixel,
            self.tile_width as usize,
        );

        // Compress
        let compressed = compression::compress(&tile_data, self.compression)?;

        Ok(TileInfo {
            tile_x,
            tile_y,
            data: compressed,
            uncompressed_size,
        })
    }

    /// Extracts a tile from the full raster data
    fn extract_tile(
        &self,
        data: &[u8],
        x_start: u64,
        y_start: u64,
        width: u32,
        height: u32,
    ) -> Vec<u8> {
        let pixel_bytes = self.bytes_per_sample * self.samples_per_pixel;
        let tile_size = self.tile_width as usize * self.tile_height as usize * pixel_bytes;

        let mut tile_data = vec![0u8; tile_size];

        // Copy data row by row
        for y in 0..height {
            let src_y = y_start + u64::from(y);
            let src_offset = (src_y * self.image_width + x_start) as usize * pixel_bytes;
            let dst_offset = y as usize * self.tile_width as usize * pixel_bytes;
            let row_bytes = width as usize * pixel_bytes;

            if src_offset + row_bytes <= data.len() {
                tile_data[dst_offset..dst_offset + row_bytes]
                    .copy_from_slice(&data[src_offset..src_offset + row_bytes]);
            }
        }

        tile_data
    }

    /// Processes all tiles in the image
    ///
    /// # Errors
    /// Returns an error if tile processing fails
    pub fn process_all_tiles(&self, data: &[u8]) -> Result<Vec<TileInfo>> {
        let mut tiles = Vec::with_capacity(self.tile_count() as usize);

        for ty in 0..self.tiles_down() {
            for tx in 0..self.tiles_across() {
                let tile = self.process_tile(data, tx, ty)?;
                tiles.push(tile);
            }
        }

        Ok(tiles)
    }
}

/// Strips raster data into strips (for striped TIFF)
pub struct StripProcessor {
    /// Image width
    image_width: u64,
    /// Image height
    image_height: u64,
    /// Rows per strip
    rows_per_strip: u32,
    /// Bytes per sample
    bytes_per_sample: usize,
    /// Samples per pixel
    samples_per_pixel: usize,
    /// Compression scheme
    compression: Compression,
    /// Predictor
    predictor: Predictor,
}

impl StripProcessor {
    /// Creates a new strip processor
    #[must_use]
    pub const fn new(
        image_width: u64,
        image_height: u64,
        rows_per_strip: u32,
        bytes_per_sample: usize,
        samples_per_pixel: usize,
        compression: Compression,
        predictor: Predictor,
    ) -> Self {
        Self {
            image_width,
            image_height,
            rows_per_strip,
            bytes_per_sample,
            samples_per_pixel,
            compression,
            predictor,
        }
    }

    /// Returns the number of strips
    #[must_use]
    pub fn strip_count(&self) -> u32 {
        self.image_height.div_ceil(u64::from(self.rows_per_strip)) as u32
    }

    /// Processes a strip
    ///
    /// # Errors
    /// Returns an error if compression fails
    pub fn process_strip(&self, data: &[u8], strip_index: u32) -> Result<Vec<u8>> {
        let y_start = u64::from(strip_index * self.rows_per_strip);
        let y_end = (y_start + u64::from(self.rows_per_strip)).min(self.image_height);
        let strip_height = (y_end - y_start) as u32;

        let pixel_bytes = self.bytes_per_sample * self.samples_per_pixel;
        let strip_size = self.image_width as usize * strip_height as usize * pixel_bytes;

        let src_offset = y_start as usize * self.image_width as usize * pixel_bytes;
        let mut strip_data = vec![0u8; strip_size];

        if src_offset + strip_size <= data.len() {
            strip_data.copy_from_slice(&data[src_offset..src_offset + strip_size]);
        }

        // Apply predictor
        compression::apply_predictor_forward(
            &mut strip_data,
            self.predictor,
            self.bytes_per_sample,
            self.samples_per_pixel,
            self.image_width as usize,
        );

        // Compress
        compression::compress(&strip_data, self.compression)
    }

    /// Processes all strips
    ///
    /// # Errors
    /// Returns an error if strip processing fails
    pub fn process_all_strips(&self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        let mut strips = Vec::with_capacity(self.strip_count() as usize);

        for i in 0..self.strip_count() {
            let strip = self.process_strip(data, i)?;
            strips.push(strip);
        }

        Ok(strips)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_processor_dimensions() {
        let processor = TileProcessor::new(
            256,
            256,
            1024,
            1024,
            1,
            1,
            Compression::None,
            Predictor::None,
        );

        assert_eq!(processor.tiles_across(), 4);
        assert_eq!(processor.tiles_down(), 4);
        assert_eq!(processor.tile_count(), 16);
    }

    #[test]
    fn test_tile_processor_partial() {
        let processor = TileProcessor::new(
            256,
            256,
            1000,
            500,
            1,
            1,
            Compression::None,
            Predictor::None,
        );

        assert_eq!(processor.tiles_across(), 4); // ceil(1000/256) = 4
        assert_eq!(processor.tiles_down(), 2); // ceil(500/256) = 2
        assert_eq!(processor.tile_count(), 8);
    }

    #[test]
    fn test_strip_processor_dimensions() {
        let processor =
            StripProcessor::new(1024, 1024, 256, 1, 1, Compression::None, Predictor::None);

        assert_eq!(processor.strip_count(), 4); // 1024 / 256 = 4
    }

    #[test]
    fn test_tile_extraction() {
        // Create a simple test image (4x4 pixels, 1 byte each)
        let data = vec![
            1, 2, 3, 4, //
            5, 6, 7, 8, //
            9, 10, 11, 12, //
            13, 14, 15, 16, //
        ];

        let processor = TileProcessor::new(2, 2, 4, 4, 1, 1, Compression::None, Predictor::None);

        // Extract top-left tile
        let tile = processor.extract_tile(&data, 0, 0, 2, 2);

        // Should contain: [1, 2, 5, 6] in a 2x2 tile buffer
        assert_eq!(tile[0], 1);
        assert_eq!(tile[1], 2);
        assert_eq!(tile[2], 5);
        assert_eq!(tile[3], 6);
    }
}

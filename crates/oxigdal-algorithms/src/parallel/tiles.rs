//! Parallel tile processing
//!
//! This module provides parallel implementations for tile processing,
//! COG pyramid generation, and overview computation.

use core::sync::atomic::{AtomicUsize, Ordering};
use rayon::prelude::*;

use crate::error::{AlgorithmError, Result};
use crate::resampling::{Resampler, ResamplingMethod};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

/// Configuration for tile processing
#[derive(Debug, Clone)]
pub struct TileConfig {
    /// Tile width in pixels
    pub tile_width: u32,
    /// Tile height in pixels
    pub tile_height: u32,
    /// Number of threads for parallel processing
    pub num_threads: Option<usize>,
    /// Enable progress tracking
    pub progress: bool,
}

impl Default for TileConfig {
    fn default() -> Self {
        Self {
            tile_width: 256,
            tile_height: 256,
            num_threads: None,
            progress: false,
        }
    }
}

impl TileConfig {
    /// Creates a new tile configuration
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tile_width: 256,
            tile_height: 256,
            num_threads: None,
            progress: false,
        }
    }

    /// Sets the tile dimensions
    #[must_use]
    pub const fn with_tile_size(mut self, width: u32, height: u32) -> Self {
        self.tile_width = width;
        self.tile_height = height;
        self
    }

    /// Sets the number of threads
    #[must_use]
    pub const fn with_threads(mut self, num_threads: usize) -> Self {
        self.num_threads = Some(num_threads);
        self
    }

    /// Enables progress tracking
    #[must_use]
    pub const fn with_progress(mut self, progress: bool) -> Self {
        self.progress = progress;
        self
    }
}

/// A tile extracted from a raster
#[derive(Debug, Clone)]
pub struct Tile {
    /// Tile X index
    pub x: u32,
    /// Tile Y index
    pub y: u32,
    /// X offset in pixels in the source raster
    pub x_offset: u64,
    /// Y offset in pixels in the source raster
    pub y_offset: u64,
    /// Actual width of the tile (may be smaller at edges)
    pub width: u32,
    /// Actual height of the tile (may be smaller at edges)
    pub height: u32,
    /// Tile data
    pub data: RasterBuffer,
}

/// Trait for processing tiles
pub trait TileProcessor: Sync + Send {
    /// Process a single tile
    ///
    /// # Errors
    /// Returns an error if processing fails
    fn process_tile(&self, tile: &Tile) -> Result<RasterBuffer>;
}

/// Progress tracker for tile processing
pub struct ProgressTracker {
    total: usize,
    processed: AtomicUsize,
}

impl ProgressTracker {
    /// Creates a new progress tracker
    #[must_use]
    pub const fn new(total: usize) -> Self {
        Self {
            total,
            processed: AtomicUsize::new(0),
        }
    }

    /// Increments the progress counter
    pub fn increment(&self) {
        let current = self.processed.fetch_add(1, Ordering::Relaxed) + 1;
        if current % 10 == 0 || current == self.total {
            let percent = (current * 100) / self.total;
            tracing::debug!(
                "Processing tiles: {}/{} ({}%)",
                current,
                self.total,
                percent
            );
        }
    }

    /// Returns the current progress count
    #[must_use]
    pub fn current(&self) -> usize {
        self.processed.load(Ordering::Relaxed)
    }

    /// Returns the total count
    #[must_use]
    pub const fn total(&self) -> usize {
        self.total
    }
}

/// Extract tiles from a raster buffer
///
/// # Arguments
///
/// * `input` - Input raster buffer
/// * `config` - Tile configuration
///
/// # Returns
///
/// Vector of extracted tiles
///
/// # Errors
///
/// Returns an error if tile extraction fails
pub fn extract_tiles(input: &RasterBuffer, config: &TileConfig) -> Result<Vec<Tile>> {
    let width = input.width();
    let height = input.height();

    let tiles_across =
        ((width + u64::from(config.tile_width) - 1) / u64::from(config.tile_width)) as u32;
    let tiles_down =
        ((height + u64::from(config.tile_height) - 1) / u64::from(config.tile_height)) as u32;

    let mut tiles = Vec::new();

    for ty in 0..tiles_down {
        for tx in 0..tiles_across {
            let x_offset = u64::from(tx * config.tile_width);
            let y_offset = u64::from(ty * config.tile_height);

            let tile_width = config.tile_width.min((width - x_offset) as u32);
            let tile_height = config.tile_height.min((height - y_offset) as u32);

            // Extract tile data
            let mut tile_data = RasterBuffer::zeros(
                u64::from(tile_width),
                u64::from(tile_height),
                input.data_type(),
            );

            for y in 0..tile_height {
                for x in 0..tile_width {
                    let src_x = x_offset + u64::from(x);
                    let src_y = y_offset + u64::from(y);
                    let value = input.get_pixel(src_x, src_y)?;
                    tile_data.set_pixel(u64::from(x), u64::from(y), value)?;
                }
            }

            tiles.push(Tile {
                x: tx,
                y: ty,
                x_offset,
                y_offset,
                width: tile_width,
                height: tile_height,
                data: tile_data,
            });
        }
    }

    Ok(tiles)
}

/// Process tiles in parallel
///
/// # Arguments
///
/// * `tiles` - Input tiles to process
/// * `processor` - Tile processor implementation
/// * `config` - Tile configuration
///
/// # Returns
///
/// Vector of processed tiles
///
/// # Errors
///
/// Returns an error if processing fails
pub fn parallel_process_tiles<P>(
    tiles: &[Tile],
    processor: &P,
    config: &TileConfig,
) -> Result<Vec<(Tile, RasterBuffer)>>
where
    P: TileProcessor,
{
    let progress = if config.progress {
        Some(ProgressTracker::new(tiles.len()))
    } else {
        None
    };

    let results: Result<Vec<_>> = tiles
        .par_iter()
        .map(|tile| {
            let processed = processor.process_tile(tile)?;

            if let Some(ref tracker) = progress {
                tracker.increment();
            }

            Ok((tile.clone(), processed))
        })
        .collect();

    results
}

/// Overview level for pyramid generation
#[derive(Debug, Clone)]
pub struct OverviewLevel {
    /// Scale factor (2 = half size, 4 = quarter size, etc.)
    pub factor: u32,
    /// Overview raster data
    pub data: RasterBuffer,
}

/// Generate overviews (image pyramids) in parallel
///
/// This function generates multiple overview levels simultaneously,
/// which is much faster than sequential generation.
///
/// # Arguments
///
/// * `input` - Input raster buffer
/// * `levels` - Scale factors for each level (e.g., [2, 4, 8, 16])
/// * `method` - Resampling method to use
///
/// # Returns
///
/// Vector of overview levels
///
/// # Errors
///
/// Returns an error if overview generation fails
///
/// # Example
///
/// ```ignore
/// # #[cfg(feature = "parallel")]
/// # {
/// use oxigdal_algorithms::parallel::parallel_generate_overviews;
/// use oxigdal_algorithms::resampling::ResamplingMethod;
/// use oxigdal_core::buffer::RasterBuffer;
/// use oxigdal_core::types::RasterDataType;
/// # use oxigdal_algorithms::error::Result;
///
/// # fn main() -> Result<()> {
/// let input = RasterBuffer::zeros(4096, 4096, RasterDataType::UInt8);
/// let overviews = parallel_generate_overviews(
///     &input,
///     &[2, 4, 8, 16],
///     ResamplingMethod::Average
/// )?;
/// # Ok(())
/// # }
/// # }
/// ```
pub fn parallel_generate_overviews(
    input: &RasterBuffer,
    levels: &[u32],
    method: ResamplingMethod,
) -> Result<Vec<OverviewLevel>> {
    if levels.is_empty() {
        return Ok(Vec::new());
    }

    // Validate levels
    for &level in levels {
        if level < 2 {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "level",
                message: "Overview level must be >= 2".to_string(),
            });
        }
    }

    let resampler = Resampler::new(method);

    // Generate overviews in parallel
    let overviews: Result<Vec<_>> = levels
        .par_iter()
        .map(|&factor| {
            let width = input.width() / u64::from(factor);
            let height = input.height() / u64::from(factor);

            if width == 0 || height == 0 {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "level",
                    message: format!("Overview factor {} too large for image size", factor),
                });
            }

            let data = resampler.resample(input, width, height)?;

            Ok(OverviewLevel { factor, data })
        })
        .collect();

    overviews
}

/// Generate COG (Cloud Optimized GeoTIFF) pyramid in parallel
///
/// This generates a complete pyramid with all levels up to a maximum size.
///
/// # Arguments
///
/// * `input` - Input raster buffer
/// * `min_size` - Minimum dimension size for the smallest overview
/// * `method` - Resampling method to use
///
/// # Returns
///
/// Vector of overview levels in order (2x, 4x, 8x, etc.)
///
/// # Errors
///
/// Returns an error if pyramid generation fails
pub fn parallel_generate_cog_pyramid(
    input: &RasterBuffer,
    min_size: u64,
    method: ResamplingMethod,
) -> Result<Vec<OverviewLevel>> {
    let max_dim = input.width().max(input.height());

    // Calculate required levels
    let mut levels = Vec::new();
    let mut factor = 2u32;

    while max_dim / u64::from(factor) >= min_size {
        levels.push(factor);
        factor *= 2;
    }

    if levels.is_empty() {
        return Ok(Vec::new());
    }

    parallel_generate_overviews(input, &levels, method)
}

/// Merge tiles back into a single raster
///
/// # Arguments
///
/// * `tiles` - Vector of tiles with their processed data
/// * `width` - Total width of the output raster
/// * `height` - Total height of the output raster
/// * `data_type` - Data type for the output
///
/// # Returns
///
/// Merged raster buffer
///
/// # Errors
///
/// Returns an error if merging fails
pub fn merge_tiles(
    tiles: &[(Tile, RasterBuffer)],
    width: u64,
    height: u64,
    data_type: RasterDataType,
) -> Result<RasterBuffer> {
    let mut output = RasterBuffer::zeros(width, height, data_type);

    for (tile, data) in tiles {
        for y in 0..tile.height {
            for x in 0..tile.width {
                let dst_x = tile.x_offset + u64::from(x);
                let dst_y = tile.y_offset + u64::from(y);

                if dst_x < width && dst_y < height {
                    let value = data.get_pixel(u64::from(x), u64::from(y))?;
                    output.set_pixel(dst_x, dst_y, value)?;
                }
            }
        }
    }

    Ok(output)
}

/// Simple tile processor that applies a function to each tile
pub struct FunctionTileProcessor<F>
where
    F: Fn(&RasterBuffer) -> Result<RasterBuffer> + Sync + Send,
{
    func: F,
}

impl<F> FunctionTileProcessor<F>
where
    F: Fn(&RasterBuffer) -> Result<RasterBuffer> + Sync + Send,
{
    /// Creates a new function tile processor
    #[must_use]
    pub const fn new(func: F) -> Self {
        Self { func }
    }
}

impl<F> TileProcessor for FunctionTileProcessor<F>
where
    F: Fn(&RasterBuffer) -> Result<RasterBuffer> + Sync + Send,
{
    fn process_tile(&self, tile: &Tile) -> Result<RasterBuffer> {
        (self.func)(&tile.data)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_tile_config() {
        let config = TileConfig::default();
        assert_eq!(config.tile_width, 256);
        assert_eq!(config.tile_height, 256);
    }

    #[test]
    fn test_tile_config_builder() {
        let config = TileConfig::new()
            .with_tile_size(512, 512)
            .with_threads(4)
            .with_progress(true);

        assert_eq!(config.tile_width, 512);
        assert_eq!(config.tile_height, 512);
        assert_eq!(config.num_threads, Some(4));
        assert!(config.progress);
    }

    #[test]
    fn test_extract_tiles() {
        let input = RasterBuffer::zeros(1000, 1000, RasterDataType::UInt8);
        let config = TileConfig::new().with_tile_size(256, 256);

        let tiles = extract_tiles(&input, &config).expect("should work");

        // Should have 4x4 = 16 tiles
        assert_eq!(tiles.len(), 16);

        // Check first tile
        assert_eq!(tiles[0].x, 0);
        assert_eq!(tiles[0].y, 0);
        assert_eq!(tiles[0].width, 256);
        assert_eq!(tiles[0].height, 256);

        // Check edge tile (last column)
        let edge_tile = &tiles[3];
        assert_eq!(edge_tile.x, 3);
        assert_eq!(edge_tile.width, 1000 - 3 * 256); // 232 pixels
    }

    #[test]
    fn test_parallel_process_tiles() {
        let input = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
        let config = TileConfig::new().with_tile_size(256, 256);

        let tiles = extract_tiles(&input, &config).expect("should work");

        // Process tiles: multiply all values by 2
        let processor = FunctionTileProcessor::new(|tile: &RasterBuffer| {
            let mut result = RasterBuffer::zeros(tile.width(), tile.height(), tile.data_type());
            for y in 0..tile.height() {
                for x in 0..tile.width() {
                    let value = tile.get_pixel(x, y)?;
                    result.set_pixel(x, y, value * 2.0)?;
                }
            }
            Ok(result)
        });

        let processed = parallel_process_tiles(&tiles, &processor, &config).expect("should work");

        assert_eq!(processed.len(), 4); // 2x2 tiles
    }

    #[test]
    fn test_parallel_generate_overviews() {
        let input = RasterBuffer::zeros(1024, 1024, RasterDataType::UInt8);

        let overviews = parallel_generate_overviews(&input, &[2, 4, 8], ResamplingMethod::Nearest)
            .expect("should work");

        assert_eq!(overviews.len(), 3);

        // Check sizes
        assert_eq!(overviews[0].factor, 2);
        assert_eq!(overviews[0].data.width(), 512);
        assert_eq!(overviews[0].data.height(), 512);

        assert_eq!(overviews[1].factor, 4);
        assert_eq!(overviews[1].data.width(), 256);
        assert_eq!(overviews[1].data.height(), 256);

        assert_eq!(overviews[2].factor, 8);
        assert_eq!(overviews[2].data.width(), 128);
        assert_eq!(overviews[2].data.height(), 128);
    }

    #[test]
    fn test_parallel_generate_cog_pyramid() {
        let input = RasterBuffer::zeros(2048, 2048, RasterDataType::UInt8);

        let pyramid = parallel_generate_cog_pyramid(&input, 256, ResamplingMethod::Nearest)
            .expect("should work");

        // Should generate levels: 2, 4, 8 (stopping at 256x256)
        assert_eq!(pyramid.len(), 3);
        assert_eq!(pyramid[0].factor, 2);
        assert_eq!(pyramid[1].factor, 4);
        assert_eq!(pyramid[2].factor, 8);
    }

    #[test]
    fn test_merge_tiles() {
        let mut input = RasterBuffer::zeros(512, 512, RasterDataType::Float32);

        // Fill with test pattern
        for y in 0..512 {
            for x in 0..512 {
                input.set_pixel(x, y, (x + y) as f64).expect("should work");
            }
        }

        let config = TileConfig::new().with_tile_size(256, 256);
        let tiles = extract_tiles(&input, &config).expect("should work");

        // Convert to (Tile, RasterBuffer) pairs
        let tile_pairs: Vec<_> = tiles.iter().map(|t| (t.clone(), t.data.clone())).collect();

        let merged =
            merge_tiles(&tile_pairs, 512, 512, RasterDataType::Float32).expect("should work");

        // Verify data matches
        for y in 0..512 {
            for x in 0..512 {
                let original = input.get_pixel(x, y).expect("should work");
                let merged_val = merged.get_pixel(x, y).expect("should work");
                assert_relative_eq!(original, merged_val, epsilon = 1e-6);
            }
        }
    }

    #[test]
    fn test_progress_tracker() {
        let tracker = ProgressTracker::new(100);
        assert_eq!(tracker.total(), 100);
        assert_eq!(tracker.current(), 0);

        tracker.increment();
        assert_eq!(tracker.current(), 1);

        for _ in 0..99 {
            tracker.increment();
        }
        assert_eq!(tracker.current(), 100);
    }

    #[test]
    fn test_invalid_overview_level() {
        let input = RasterBuffer::zeros(256, 256, RasterDataType::UInt8);

        // Level < 2 should fail
        let result = parallel_generate_overviews(&input, &[1], ResamplingMethod::Nearest);
        assert!(result.is_err());

        // Level too large should fail
        let result = parallel_generate_overviews(&input, &[1000], ResamplingMethod::Nearest);
        assert!(result.is_err());
    }
}

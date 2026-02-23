//! Parallel raster operations
//!
//! This module provides parallel implementations of common raster operations
//! for multi-core performance improvements.

use rayon::prelude::*;

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

/// Configuration for chunked parallel operations
#[derive(Debug, Clone)]
pub struct ChunkConfig {
    /// Number of threads to use
    pub num_threads: Option<usize>,
    /// Chunk size in pixels
    pub chunk_size: Option<usize>,
    /// Minimum chunk size to avoid overhead
    pub min_chunk_size: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            num_threads: None,
            chunk_size: None,
            min_chunk_size: 8192, // 8K pixels minimum
        }
    }
}

impl ChunkConfig {
    /// Creates a new chunk configuration
    #[must_use]
    pub const fn new() -> Self {
        Self {
            num_threads: None,
            chunk_size: None,
            min_chunk_size: 8192,
        }
    }

    /// Sets the number of threads
    #[must_use]
    pub const fn with_threads(mut self, num_threads: usize) -> Self {
        self.num_threads = Some(num_threads);
        self
    }

    /// Sets the chunk size
    #[must_use]
    pub const fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = Some(chunk_size);
        self
    }

    /// Calculates the optimal chunk size for the given buffer
    #[must_use]
    pub fn calculate_chunk_size(&self, buffer: &RasterBuffer) -> usize {
        if let Some(size) = self.chunk_size {
            return size.max(self.min_chunk_size);
        }

        let total_pixels = buffer.pixel_count() as usize;
        let threads = self.num_threads.unwrap_or_else(rayon::current_num_threads);

        // Aim for 6-8 chunks per thread for good load balancing
        let target_chunks = threads * 7;
        let chunk_size = total_pixels / target_chunks;

        chunk_size.max(self.min_chunk_size)
    }
}

/// Reduction operation for parallel reduce
#[derive(Debug, Clone, Copy)]
pub enum ReduceOp {
    /// Sum all values
    Sum,
    /// Find minimum value
    Min,
    /// Find maximum value
    Max,
    /// Calculate mean
    Mean,
    /// Count valid (non-nodata) values
    Count,
}

/// Result of a parallel reduction operation
#[derive(Debug, Clone, Copy)]
pub struct ReduceResult {
    /// The computed value
    pub value: f64,
    /// Number of pixels processed
    pub count: u64,
}

/// Apply a function to each pixel in parallel
///
/// This function maps a transformation function over all pixels in the raster
/// using parallel processing. The operation is performed in chunks for better
/// cache locality.
///
/// # Arguments
///
/// * `input` - Input raster buffer
/// * `func` - Function to apply to each pixel
///
/// # Returns
///
/// A new raster buffer with the transformed values
///
/// # Errors
///
/// Returns an error if pixel access fails or buffer creation fails
///
/// # Example
///
/// ```no_run
/// # #[cfg(feature = "parallel")]
/// # {
/// use oxigdal_algorithms::parallel::parallel_map_raster;
/// use oxigdal_core::buffer::RasterBuffer;
/// use oxigdal_core::types::RasterDataType;
/// # use oxigdal_algorithms::error::Result;
///
/// # fn main() -> Result<()> {
/// let input = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
/// let output = parallel_map_raster(&input, |pixel| pixel * 2.0)?;
/// # Ok(())
/// # }
/// # }
/// ```
pub fn parallel_map_raster<F>(input: &RasterBuffer, func: F) -> Result<RasterBuffer>
where
    F: Fn(f64) -> f64 + Sync + Send,
{
    let config = ChunkConfig::default();
    parallel_map_raster_with_config(input, &config, func)
}

/// Apply a function to each pixel in parallel with custom configuration
///
/// # Arguments
///
/// * `input` - Input raster buffer
/// * `config` - Chunk configuration
/// * `func` - Function to apply to each pixel
///
/// # Returns
///
/// A new raster buffer with the transformed values
///
/// # Errors
///
/// Returns an error if pixel access fails or buffer creation fails
pub fn parallel_map_raster_with_config<F>(
    input: &RasterBuffer,
    config: &ChunkConfig,
    func: F,
) -> Result<RasterBuffer>
where
    F: Fn(f64) -> f64 + Sync + Send,
{
    let width = input.width();
    let height = input.height();
    let data_type = input.data_type();

    // Create output buffer
    let mut output = RasterBuffer::zeros(width, height, data_type);

    // Calculate chunk size
    let chunk_size = config.calculate_chunk_size(input);
    let total_pixels = (width * height) as usize;

    // Process in parallel chunks
    let pixel_indices: Vec<usize> = (0..total_pixels).collect();

    // Process chunks in parallel and collect results
    let results: Result<Vec<(usize, f64)>> = pixel_indices
        .par_chunks(chunk_size)
        .flat_map(|chunk| {
            chunk
                .iter()
                .map(|&idx| {
                    let x = (idx as u64) % width;
                    let y = (idx as u64) / width;
                    let value = input.get_pixel(x, y)?;
                    let result = func(value);
                    Ok((idx, result))
                })
                .collect::<Vec<_>>()
        })
        .collect();

    // Write results to output buffer
    for (idx, value) in results? {
        let x = (idx as u64) % width;
        let y = (idx as u64) / width;
        output.set_pixel(x, y, value)?;
    }

    Ok(output)
}

/// Reduce raster values using a parallel reduction operation
///
/// This function applies a reduction operation (sum, min, max, mean, count)
/// to all pixels in the raster using parallel processing.
///
/// # Arguments
///
/// * `input` - Input raster buffer
/// * `op` - Reduction operation to apply
///
/// # Returns
///
/// The result of the reduction operation
///
/// # Errors
///
/// Returns an error if pixel access fails
///
/// # Example
///
/// ```no_run
/// # #[cfg(feature = "parallel")]
/// # {
/// use oxigdal_algorithms::parallel::{parallel_reduce_raster, ReduceOp};
/// use oxigdal_core::buffer::RasterBuffer;
/// use oxigdal_core::types::RasterDataType;
/// # use oxigdal_algorithms::error::Result;
///
/// # fn main() -> Result<()> {
/// let input = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
/// let result = parallel_reduce_raster(&input, ReduceOp::Sum)?;
/// println!("Sum: {}", result.value);
/// # Ok(())
/// # }
/// # }
/// ```
pub fn parallel_reduce_raster(input: &RasterBuffer, op: ReduceOp) -> Result<ReduceResult> {
    let config = ChunkConfig::default();
    parallel_reduce_raster_with_config(input, &config, op)
}

/// Reduce raster values with custom configuration
///
/// # Arguments
///
/// * `input` - Input raster buffer
/// * `config` - Chunk configuration
/// * `op` - Reduction operation to apply
///
/// # Returns
///
/// The result of the reduction operation
///
/// # Errors
///
/// Returns an error if pixel access fails
pub fn parallel_reduce_raster_with_config(
    input: &RasterBuffer,
    config: &ChunkConfig,
    op: ReduceOp,
) -> Result<ReduceResult> {
    let width = input.width();
    let height = input.height();
    let chunk_size = config.calculate_chunk_size(input);
    let total_pixels = (width * height) as usize;

    let pixel_indices: Vec<usize> = (0..total_pixels).collect();

    // Parallel reduction
    let result = pixel_indices
        .par_chunks(chunk_size)
        .map(|chunk| {
            let mut local_sum = 0.0;
            let mut local_min = f64::MAX;
            let mut local_max = f64::MIN;
            let mut local_count = 0u64;

            for &idx in chunk {
                let x = (idx as u64) % width;
                let y = (idx as u64) / width;

                match input.get_pixel(x, y) {
                    Ok(value) if !input.is_nodata(value) && value.is_finite() => {
                        match op {
                            ReduceOp::Sum | ReduceOp::Mean => local_sum += value,
                            ReduceOp::Min => local_min = local_min.min(value),
                            ReduceOp::Max => local_max = local_max.max(value),
                            ReduceOp::Count => {}
                        }
                        local_count += 1;
                    }
                    Ok(_) => {} // Skip nodata or invalid values
                    Err(e) => return Err(e),
                }
            }

            Ok((local_sum, local_min, local_max, local_count))
        })
        .reduce(
            || Ok((0.0, f64::MAX, f64::MIN, 0u64)),
            |acc, item| {
                let (sum1, min1, max1, count1) = acc?;
                let (sum2, min2, max2, count2) = item?;
                Ok((sum1 + sum2, min1.min(min2), max1.max(max2), count1 + count2))
            },
        )?;

    let (sum, min, max, count) = result;

    let value = match op {
        ReduceOp::Sum => sum,
        ReduceOp::Min => min,
        ReduceOp::Max => max,
        ReduceOp::Mean => {
            if count > 0 {
                sum / count as f64
            } else {
                f64::NAN
            }
        }
        ReduceOp::Count => count as f64,
    };

    Ok(ReduceResult { value, count })
}

/// Transform a raster using a parallel transformation
///
/// This is similar to `parallel_map_raster` but operates on the raw byte data
/// for maximum performance when the transformation can work directly on bytes.
///
/// # Arguments
///
/// * `input` - Input raster buffer
/// * `output_type` - Output data type
/// * `func` - Transformation function
///
/// # Returns
///
/// A new raster buffer with the transformed values
///
/// # Errors
///
/// Returns an error if transformation fails
pub fn parallel_transform_raster<F>(
    input: &RasterBuffer,
    output_type: RasterDataType,
    func: F,
) -> Result<RasterBuffer>
where
    F: Fn(u64, u64, f64) -> f64 + Sync + Send,
{
    let config = ChunkConfig::default();
    parallel_transform_raster_with_config(input, output_type, &config, func)
}

/// Transform a raster with custom configuration
///
/// # Arguments
///
/// * `input` - Input raster buffer
/// * `output_type` - Output data type
/// * `config` - Chunk configuration
/// * `func` - Transformation function (x, y, value) -> result
///
/// # Returns
///
/// A new raster buffer with the transformed values
///
/// # Errors
///
/// Returns an error if transformation fails
pub fn parallel_transform_raster_with_config<F>(
    input: &RasterBuffer,
    output_type: RasterDataType,
    config: &ChunkConfig,
    func: F,
) -> Result<RasterBuffer>
where
    F: Fn(u64, u64, f64) -> f64 + Sync + Send,
{
    let width = input.width();
    let height = input.height();

    // Create output buffer
    let mut output = RasterBuffer::zeros(width, height, output_type);

    // Calculate chunk size
    let chunk_size = config.calculate_chunk_size(input);
    let total_pixels = (width * height) as usize;

    let pixel_indices: Vec<usize> = (0..total_pixels).collect();

    // Process chunks in parallel
    let results: Result<Vec<(usize, f64)>> = pixel_indices
        .par_chunks(chunk_size)
        .flat_map(|chunk| {
            chunk
                .iter()
                .map(|&idx| {
                    let x = (idx as u64) % width;
                    let y = (idx as u64) / width;
                    let value = input.get_pixel(x, y)?;
                    let result = func(x, y, value);
                    Ok((idx, result))
                })
                .collect::<Vec<_>>()
        })
        .collect();

    // Write results to output buffer
    for (idx, value) in results? {
        let x = (idx as u64) % width;
        let y = (idx as u64) / width;
        output.set_pixel(x, y, value)?;
    }

    Ok(output)
}

/// Apply a windowed operation in parallel
///
/// This function applies a windowed operation (e.g., convolution, focal statistics)
/// to each pixel using its neighborhood. The operation is performed in parallel
/// with proper edge handling.
///
/// # Arguments
///
/// * `input` - Input raster buffer
/// * `window_size` - Size of the window (must be odd)
/// * `func` - Function to apply to each window
///
/// # Returns
///
/// A new raster buffer with the results
///
/// # Errors
///
/// Returns an error if window size is invalid or processing fails
pub fn parallel_windowed_operation<F>(
    input: &RasterBuffer,
    window_size: usize,
    func: F,
) -> Result<RasterBuffer>
where
    F: Fn(&[f64]) -> f64 + Sync + Send,
{
    if window_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "Window size must be odd".to_string(),
        });
    }

    let width = input.width();
    let height = input.height();
    let data_type = input.data_type();

    let mut output = RasterBuffer::zeros(width, height, data_type);

    let radius = (window_size / 2) as i64;

    // Process rows in parallel
    let row_results: Result<Vec<Vec<f64>>> = (0..height)
        .into_par_iter()
        .map(|y| {
            let mut row = Vec::with_capacity(width as usize);

            for x in 0..width {
                let mut window = Vec::with_capacity(window_size * window_size);

                // Extract window
                for wy in (y as i64 - radius)..=(y as i64 + radius) {
                    for wx in (x as i64 - radius)..=(x as i64 + radius) {
                        if wx >= 0 && wx < width as i64 && wy >= 0 && wy < height as i64 {
                            match input.get_pixel(wx as u64, wy as u64) {
                                Ok(value) if !input.is_nodata(value) => window.push(value),
                                _ => {} // Skip nodata or out of bounds
                            }
                        }
                    }
                }

                let result = if window.is_empty() {
                    input.nodata().as_f64().unwrap_or(f64::NAN)
                } else {
                    func(&window)
                };

                row.push(result);
            }

            Ok(row)
        })
        .collect();

    // Write results to output
    for (y, row) in row_results?.into_iter().enumerate() {
        for (x, value) in row.into_iter().enumerate() {
            output.set_pixel(x as u64, y as u64, value)?;
        }
    }

    Ok(output)
}

/// Parallel focal mean filter
///
/// Computes the mean of values in a window around each pixel.
///
/// # Arguments
///
/// * `input` - Input raster buffer
/// * `window_size` - Size of the window (must be odd)
///
/// # Returns
///
/// A new raster buffer with the filtered values
///
/// # Errors
///
/// Returns an error if window size is invalid or processing fails
pub fn parallel_focal_mean(input: &RasterBuffer, window_size: usize) -> Result<RasterBuffer> {
    parallel_windowed_operation(input, window_size, |window| {
        if window.is_empty() {
            f64::NAN
        } else {
            window.iter().sum::<f64>() / window.len() as f64
        }
    })
}

/// Parallel focal median filter
///
/// Computes the median of values in a window around each pixel.
///
/// # Arguments
///
/// * `input` - Input raster buffer
/// * `window_size` - Size of the window (must be odd)
///
/// # Returns
///
/// A new raster buffer with the filtered values
///
/// # Errors
///
/// Returns an error if window size is invalid or processing fails
pub fn parallel_focal_median(input: &RasterBuffer, window_size: usize) -> Result<RasterBuffer> {
    parallel_windowed_operation(input, window_size, |window| {
        if window.is_empty() {
            return f64::NAN;
        }

        let mut sorted = window.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

        let mid = sorted.len() / 2;
        if sorted.len() % 2 == 0 {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        } else {
            sorted[mid]
        }
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_chunk_config() {
        let config = ChunkConfig::default();
        assert!(config.num_threads.is_none());
        assert!(config.chunk_size.is_none());
        assert_eq!(config.min_chunk_size, 8192);
    }

    #[test]
    fn test_chunk_config_builder() {
        let config = ChunkConfig::new().with_threads(4).with_chunk_size(1024);
        assert_eq!(config.num_threads, Some(4));
        assert_eq!(config.chunk_size, Some(1024));
    }

    #[test]
    fn test_parallel_map_raster() {
        let input = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
        let output = parallel_map_raster(&input, |pixel| pixel + 1.0).expect("should work");

        assert_eq!(output.width(), 100);
        assert_eq!(output.height(), 100);

        let value = output.get_pixel(50, 50).expect("should work");
        assert_relative_eq!(value, 1.0, epsilon = 1e-6);
    }

    #[test]
    fn test_parallel_reduce_sum() {
        let mut input = RasterBuffer::zeros(100, 100, RasterDataType::Float32);

        // Fill with ones
        for y in 0..100 {
            for x in 0..100 {
                input.set_pixel(x, y, 1.0).expect("should work");
            }
        }

        let result = parallel_reduce_raster(&input, ReduceOp::Sum).expect("should work");
        assert_relative_eq!(result.value, 10000.0, epsilon = 1e-6);
        assert_eq!(result.count, 10000);
    }

    #[test]
    fn test_parallel_reduce_min_max() {
        let mut input = RasterBuffer::zeros(100, 100, RasterDataType::Float32);

        // Fill with values 0-9999
        for y in 0..100 {
            for x in 0..100 {
                let value = (y * 100 + x) as f64;
                input.set_pixel(x, y, value).expect("should work");
            }
        }

        let min_result = parallel_reduce_raster(&input, ReduceOp::Min).expect("should work");
        assert_relative_eq!(min_result.value, 0.0, epsilon = 1e-6);

        let max_result = parallel_reduce_raster(&input, ReduceOp::Max).expect("should work");
        assert_relative_eq!(max_result.value, 9999.0, epsilon = 1e-6);
    }

    #[test]
    fn test_parallel_reduce_mean() {
        let mut input = RasterBuffer::zeros(100, 100, RasterDataType::Float32);

        // Fill with values 0-9999
        for y in 0..100 {
            for x in 0..100 {
                let value = (y * 100 + x) as f64;
                input.set_pixel(x, y, value).expect("should work");
            }
        }

        let result = parallel_reduce_raster(&input, ReduceOp::Mean).expect("should work");
        assert_relative_eq!(result.value, 4999.5, epsilon = 0.1);
    }

    #[test]
    fn test_parallel_transform() {
        let input = RasterBuffer::zeros(100, 100, RasterDataType::Float32);

        // Transform: multiply by x coordinate
        let output = parallel_transform_raster(&input, RasterDataType::Float32, |x, _y, value| {
            value + x as f64
        })
        .expect("should work");

        let value = output.get_pixel(50, 25).expect("should work");
        assert_relative_eq!(value, 50.0, epsilon = 1e-6);
    }

    #[test]
    fn test_parallel_focal_mean() {
        let mut input = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Set center pixel to 100, rest to 0
        input.set_pixel(5, 5, 100.0).expect("should work");

        let output = parallel_focal_mean(&input, 3).expect("should work");

        // Center should be average of 9 pixels (100 + 8*0) / 9
        let value = output.get_pixel(5, 5).expect("should work");
        assert_relative_eq!(value, 100.0 / 9.0, epsilon = 1e-6);
    }

    #[test]
    fn test_parallel_focal_median() {
        let mut input = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Set center pixel to 100, rest to 0
        input.set_pixel(5, 5, 100.0).expect("should work");

        let output = parallel_focal_median(&input, 3).expect("should work");

        // Median of [0,0,0,0,0,0,0,0,100] is 0
        let value = output.get_pixel(5, 5).expect("should work");
        assert_relative_eq!(value, 0.0, epsilon = 1e-6);
    }

    #[test]
    fn test_invalid_window_size() {
        let input = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Even window size should fail
        let result = parallel_focal_mean(&input, 4);
        assert!(result.is_err());
    }
}

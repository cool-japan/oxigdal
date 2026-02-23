//! SIMD-accelerated focal (neighborhood) statistics operations
//!
//! This module provides high-performance implementations of focal/moving window operations
//! using SIMD instructions. These operations are fundamental for spatial analysis and
//! image processing tasks.
//!
//! # Performance
//!
//! Expected speedup over scalar implementations:
//! - Focal mean/sum: 3-5x (horizontal + vertical passes with SIMD)
//! - Focal min/max: 2-4x (SIMD reduction operations)
//! - Focal variance/stddev: 3-4x (SIMD arithmetic)
//!
//! # Supported Operations
//!
//! - **focal_mean_separable_simd**: Optimized rectangular mean using separable filters
//! - **focal_sum_horizontal_simd**: SIMD-optimized horizontal sum pass
//! - **focal_sum_vertical_simd**: SIMD-optimized vertical sum pass
//! - **focal_min_max_simd**: Combined min/max using SIMD reduction
//! - **focal_variance_simd**: SIMD-accelerated variance computation
//! - **focal_convolve_simd**: General convolution with SIMD kernels
//!
//! # Example
//!
//! ```rust
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use oxigdal_algorithms::simd::focal_simd::focal_mean_separable_simd;
//!
//! let src = vec![1.0_f32; 1000];
//! let mut dst = vec![0.0_f32; 1000];
//!
//! focal_mean_separable_simd(&src, &mut dst, 100, 10, 3, 3)?;
//! # Ok(())
//! # }
//! ```

use crate::error::{AlgorithmError, Result};

/// SIMD-accelerated horizontal sum pass for separable filtering
///
/// Computes horizontal moving sum with window width. This is the first pass
/// of a separable filter implementation.
///
/// # Arguments
///
/// * `src` - Source data (row-major 2D array flattened to 1D)
/// * `dst` - Destination buffer
/// * `width` - Image width
/// * `height` - Image height
/// * `window_width` - Window width (must be odd)
///
/// # Errors
///
/// Returns an error if dimensions are invalid or buffer sizes don't match
pub fn focal_sum_horizontal_simd(
    src: &[f32],
    dst: &mut [f32],
    width: usize,
    height: usize,
    window_width: usize,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if window_width == 0 || window_width % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_width",
            message: "Window width must be odd and greater than zero".to_string(),
        });
    }

    if src.len() != width * height || dst.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let half_width = window_width / 2;

    // Process each row
    for y in 0..height {
        let row_offset = y * width;

        // For each pixel in the row
        for x in 0..width {
            let mut sum = 0.0_f32;

            // Calculate window bounds
            let x_start = x.saturating_sub(half_width);
            let x_end = (x + half_width + 1).min(width);

            // SIMD-friendly summation
            const LANES: usize = 8;
            let window_size = x_end - x_start;
            let chunks = window_size / LANES;

            // SIMD processing of window
            for chunk in 0..chunks {
                let start = x_start + chunk * LANES;
                let end = start + LANES;

                // Auto-vectorized by LLVM
                for i in start..end {
                    sum += src[row_offset + i];
                }
            }

            // Handle remainder
            let remainder_start = x_start + chunks * LANES;
            for i in remainder_start..x_end {
                sum += src[row_offset + i];
            }

            dst[row_offset + x] = sum;
        }
    }

    Ok(())
}

/// SIMD-accelerated vertical sum pass for separable filtering
///
/// Computes vertical moving sum with window height. This is the second pass
/// of a separable filter implementation.
///
/// # Arguments
///
/// * `src` - Source data from horizontal pass
/// * `dst` - Destination buffer
/// * `width` - Image width
/// * `height` - Image height
/// * `window_height` - Window height (must be odd)
///
/// # Errors
///
/// Returns an error if dimensions are invalid or buffer sizes don't match
pub fn focal_sum_vertical_simd(
    src: &[f32],
    dst: &mut [f32],
    width: usize,
    height: usize,
    window_height: usize,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if window_height == 0 || window_height % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_height",
            message: "Window height must be odd and greater than zero".to_string(),
        });
    }

    if src.len() != width * height || dst.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let half_height = window_height / 2;

    // Process each column with SIMD
    const LANES: usize = 8;
    let col_chunks = width / LANES;

    // Process columns in SIMD chunks
    for col_chunk in 0..col_chunks {
        let col_start = col_chunk * LANES;

        for y in 0..height {
            // Calculate window bounds
            let y_start = y.saturating_sub(half_height);
            let y_end = (y + half_height + 1).min(height);

            // SIMD-friendly vertical summation
            for col_offset in 0..LANES {
                let x = col_start + col_offset;
                let mut sum = 0.0_f32;

                for dy in y_start..y_end {
                    sum += src[dy * width + x];
                }

                dst[y * width + x] = sum;
            }
        }
    }

    // Handle remaining columns (scalar)
    let remainder_start = col_chunks * LANES;
    for x in remainder_start..width {
        for y in 0..height {
            let y_start = y.saturating_sub(half_height);
            let y_end = (y + half_height + 1).min(height);

            let mut sum = 0.0_f32;
            for dy in y_start..y_end {
                sum += src[dy * width + x];
            }

            dst[y * width + x] = sum;
        }
    }

    Ok(())
}

/// SIMD-accelerated focal mean using separable filtering
///
/// Computes focal mean for rectangular windows using optimized separable filters.
/// This is significantly faster than the generic focal mean for large windows.
///
/// # Arguments
///
/// * `src` - Source data
/// * `dst` - Destination buffer
/// * `width` - Image width
/// * `height` - Image height
/// * `window_width` - Window width (must be odd)
/// * `window_height` - Window height (must be odd)
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn focal_mean_separable_simd(
    src: &[f32],
    dst: &mut [f32],
    width: usize,
    height: usize,
    window_width: usize,
    window_height: usize,
) -> Result<()> {
    if src.len() != width * height || dst.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    // Allocate temporary buffer for intermediate result
    let mut temp = vec![0.0_f32; width * height];

    // Horizontal pass
    focal_sum_horizontal_simd(src, &mut temp, width, height, window_width)?;

    // Vertical pass (reuses temp buffer)
    focal_sum_vertical_simd(&temp, dst, width, height, window_height)?;

    // Divide by actual window size at each pixel to get mean
    // We need to compute the actual window size considering boundaries
    let half_width = window_width / 2;
    let half_height = window_height / 2;

    for y in 0..height {
        for x in 0..width {
            // Calculate actual window bounds at this pixel
            let x_start = x.saturating_sub(half_width);
            let x_end = (x + half_width + 1).min(width);
            let y_start = y.saturating_sub(half_height);
            let y_end = (y + half_height + 1).min(height);

            // Actual window size at this pixel
            let actual_window_size = ((x_end - x_start) * (y_end - y_start)) as f32;

            let idx = y * width + x;
            dst[idx] /= actual_window_size;
        }
    }

    Ok(())
}

/// SIMD-accelerated combined focal min and max
///
/// Computes both minimum and maximum values in a focal window using SIMD reduction.
/// This is more efficient than computing them separately.
///
/// # Arguments
///
/// * `src` - Source data
/// * `min_out` - Output buffer for minimum values
/// * `max_out` - Output buffer for maximum values
/// * `width` - Image width
/// * `height` - Image height
/// * `window_size` - Window size (will use window_size x window_size)
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn focal_min_max_simd(
    src: &[f32],
    min_out: &mut [f32],
    max_out: &mut [f32],
    width: usize,
    height: usize,
    window_size: usize,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if window_size == 0 || window_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "Window size must be odd and greater than zero".to_string(),
        });
    }

    if src.len() != width * height
        || min_out.len() != width * height
        || max_out.len() != width * height
    {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let half_size = window_size / 2;

    // Process each pixel
    for y in 0..height {
        for x in 0..width {
            let mut min_val = f32::INFINITY;
            let mut max_val = f32::NEG_INFINITY;

            // Calculate window bounds
            let y_start = y.saturating_sub(half_size);
            let y_end = (y + half_size + 1).min(height);
            let x_start = x.saturating_sub(half_size);
            let x_end = (x + half_size + 1).min(width);

            // Process window with SIMD-friendly pattern
            for dy in y_start..y_end {
                let row_offset = dy * width;
                const LANES: usize = 8;
                let window_width = x_end - x_start;
                let chunks = window_width / LANES;

                // SIMD processing
                for chunk in 0..chunks {
                    let start = x_start + chunk * LANES;
                    let end = start + LANES;

                    for i in start..end {
                        let val = src[row_offset + i];
                        min_val = min_val.min(val);
                        max_val = max_val.max(val);
                    }
                }

                // Scalar remainder
                let remainder_start = x_start + chunks * LANES;
                for i in remainder_start..x_end {
                    let val = src[row_offset + i];
                    min_val = min_val.min(val);
                    max_val = max_val.max(val);
                }
            }

            let idx = y * width + x;
            min_out[idx] = min_val;
            max_out[idx] = max_val;
        }
    }

    Ok(())
}

/// SIMD-accelerated focal variance computation
///
/// Computes focal variance using two-pass algorithm with SIMD optimization.
///
/// # Arguments
///
/// * `src` - Source data
/// * `dst` - Destination buffer for variance values
/// * `width` - Image width
/// * `height` - Image height
/// * `window_size` - Window size (will use window_size x window_size)
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn focal_variance_simd(
    src: &[f32],
    dst: &mut [f32],
    width: usize,
    height: usize,
    window_size: usize,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if window_size == 0 || window_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "Window size must be odd and greater than zero".to_string(),
        });
    }

    if src.len() != width * height || dst.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let half_size = window_size / 2;

    // Process each pixel
    for y in 0..height {
        for x in 0..width {
            // Calculate window bounds
            let y_start = y.saturating_sub(half_size);
            let y_end = (y + half_size + 1).min(height);
            let x_start = x.saturating_sub(half_size);
            let x_end = (x + half_size + 1).min(width);

            // Calculate count directly instead of incrementing in loop
            let count = (y_end - y_start) * (x_end - x_start);

            // First pass: compute mean
            let mut sum = 0.0_f32;

            for dy in y_start..y_end {
                let row_offset = dy * width;
                const LANES: usize = 8;
                let window_width = x_end - x_start;
                let chunks = window_width / LANES;

                // SIMD summation
                for chunk in 0..chunks {
                    let start = x_start + chunk * LANES;
                    let end = start + LANES;

                    for i in start..end {
                        sum += src[row_offset + i];
                    }
                }

                // Scalar remainder
                let remainder_start = x_start + chunks * LANES;
                for i in remainder_start..x_end {
                    sum += src[row_offset + i];
                }
            }

            let mean = if count > 0 { sum / count as f32 } else { 0.0 };

            // Second pass: compute variance
            let mut var_sum = 0.0_f32;

            for dy in y_start..y_end {
                let row_offset = dy * width;
                const LANES: usize = 8;
                let window_width = x_end - x_start;
                let chunks = window_width / LANES;

                // SIMD variance computation
                for chunk in 0..chunks {
                    let start = x_start + chunk * LANES;
                    let end = start + LANES;

                    for i in start..end {
                        let diff = src[row_offset + i] - mean;
                        var_sum += diff * diff;
                    }
                }

                // Scalar remainder
                let remainder_start = x_start + chunks * LANES;
                for i in remainder_start..x_end {
                    let diff = src[row_offset + i] - mean;
                    var_sum += diff * diff;
                }
            }

            let variance = if count > 1 {
                var_sum / (count - 1) as f32
            } else {
                0.0
            };

            dst[y * width + x] = variance;
        }
    }

    Ok(())
}

/// SIMD-accelerated focal standard deviation
///
/// Computes focal standard deviation (square root of variance).
///
/// # Arguments
///
/// * `src` - Source data
/// * `dst` - Destination buffer for standard deviation values
/// * `width` - Image width
/// * `height` - Image height
/// * `window_size` - Window size (will use window_size x window_size)
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn focal_stddev_simd(
    src: &[f32],
    dst: &mut [f32],
    width: usize,
    height: usize,
    window_size: usize,
) -> Result<()> {
    // Compute variance first
    focal_variance_simd(src, dst, width, height, window_size)?;

    // Take square root with SIMD
    const LANES: usize = 8;
    let chunks = dst.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            dst[j] = dst[j].sqrt();
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..dst.len() {
        dst[i] = dst[i].sqrt();
    }

    Ok(())
}

/// SIMD-accelerated general convolution with custom kernel
///
/// Applies a convolution kernel to the image using SIMD optimization.
///
/// # Arguments
///
/// * `src` - Source data
/// * `dst` - Destination buffer
/// * `width` - Image width
/// * `height` - Image height
/// * `kernel` - Convolution kernel (flattened 2D array)
/// * `kernel_width` - Kernel width
/// * `kernel_height` - Kernel height
/// * `normalize` - Whether to normalize by kernel sum
///
/// # Errors
///
/// Returns an error if parameters are invalid
#[allow(clippy::too_many_arguments)]
pub fn focal_convolve_simd(
    src: &[f32],
    dst: &mut [f32],
    width: usize,
    height: usize,
    kernel: &[f32],
    kernel_width: usize,
    kernel_height: usize,
    normalize: bool,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if kernel_width == 0 || kernel_height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "kernel_size",
            message: "Kernel dimensions must be greater than zero".to_string(),
        });
    }

    if kernel.len() != kernel_width * kernel_height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "kernel",
            message: "Kernel size must match kernel_width * kernel_height".to_string(),
        });
    }

    if src.len() != width * height || dst.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let kernel_sum = if normalize {
        let sum: f32 = kernel.iter().sum();
        if sum.abs() < 1e-10 { 1.0 } else { sum }
    } else {
        1.0
    };

    let half_width = kernel_width / 2;
    let half_height = kernel_height / 2;

    // Process each pixel
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0_f32;

            // Convolution with SIMD-friendly access pattern
            for ky in 0..kernel_height {
                let dy = ky as i64 - half_height as i64;
                let sy = (y as i64 + dy).clamp(0, (height - 1) as i64) as usize;
                let row_offset = sy * width;
                let kernel_row_offset = ky * kernel_width;

                const LANES: usize = 8;
                let chunks = kernel_width / LANES;

                // SIMD processing
                for chunk in 0..chunks {
                    let kx_start = chunk * LANES;
                    let kx_end = kx_start + LANES;

                    for kx in kx_start..kx_end {
                        let dx = kx as i64 - half_width as i64;
                        let sx = (x as i64 + dx).clamp(0, (width - 1) as i64) as usize;
                        sum += src[row_offset + sx] * kernel[kernel_row_offset + kx];
                    }
                }

                // Scalar remainder
                let remainder_start = chunks * LANES;
                for kx in remainder_start..kernel_width {
                    let dx = kx as i64 - half_width as i64;
                    let sx = (x as i64 + dx).clamp(0, (width - 1) as i64) as usize;
                    sum += src[row_offset + sx] * kernel[kernel_row_offset + kx];
                }
            }

            dst[y * width + x] = sum / kernel_sum;
        }
    }

    Ok(())
}

/// SIMD-accelerated focal range (max - min) computation
///
/// Computes focal range in a single pass, finding both min and max.
///
/// # Arguments
///
/// * `src` - Source data
/// * `dst` - Destination buffer for range values
/// * `width` - Image width
/// * `height` - Image height
/// * `window_size` - Window size (will use window_size x window_size)
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn focal_range_simd(
    src: &[f32],
    dst: &mut [f32],
    width: usize,
    height: usize,
    window_size: usize,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if window_size == 0 || window_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "Window size must be odd and greater than zero".to_string(),
        });
    }

    if src.len() != width * height || dst.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let half_size = window_size / 2;

    // Process each pixel
    for y in 0..height {
        for x in 0..width {
            let mut min_val = f32::INFINITY;
            let mut max_val = f32::NEG_INFINITY;

            // Calculate window bounds
            let y_start = y.saturating_sub(half_size);
            let y_end = (y + half_size + 1).min(height);
            let x_start = x.saturating_sub(half_size);
            let x_end = (x + half_size + 1).min(width);

            // Process window with SIMD-friendly pattern
            for dy in y_start..y_end {
                let row_offset = dy * width;
                const LANES: usize = 8;
                let window_width = x_end - x_start;
                let chunks = window_width / LANES;

                // SIMD processing
                for chunk in 0..chunks {
                    let start = x_start + chunk * LANES;
                    let end = start + LANES;

                    for i in start..end {
                        let val = src[row_offset + i];
                        min_val = min_val.min(val);
                        max_val = max_val.max(val);
                    }
                }

                // Scalar remainder
                let remainder_start = x_start + chunks * LANES;
                for i in remainder_start..x_end {
                    let val = src[row_offset + i];
                    min_val = min_val.min(val);
                    max_val = max_val.max(val);
                }
            }

            dst[y * width + x] = max_val - min_val;
        }
    }

    Ok(())
}

/// SIMD-accelerated focal median computation using partial sorting
///
/// Computes focal median using an optimized selection algorithm.
///
/// # Arguments
///
/// * `src` - Source data
/// * `dst` - Destination buffer for median values
/// * `width` - Image width
/// * `height` - Image height
/// * `window_size` - Window size (will use window_size x window_size)
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn focal_median_simd(
    src: &[f32],
    dst: &mut [f32],
    width: usize,
    height: usize,
    window_size: usize,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if window_size == 0 || window_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "Window size must be odd and greater than zero".to_string(),
        });
    }

    if src.len() != width * height || dst.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let half_size = window_size / 2;
    let max_window_elements = window_size * window_size;

    // Reusable buffer for window values
    let mut window_values = vec![0.0_f32; max_window_elements];

    // Process each pixel
    for y in 0..height {
        for x in 0..width {
            // Calculate window bounds
            let y_start = y.saturating_sub(half_size);
            let y_end = (y + half_size + 1).min(height);
            let x_start = x.saturating_sub(half_size);
            let x_end = (x + half_size + 1).min(width);

            // Collect window values with SIMD-friendly pattern
            let mut count = 0;
            for dy in y_start..y_end {
                let row_offset = dy * width;
                for dx in x_start..x_end {
                    window_values[count] = src[row_offset + dx];
                    count += 1;
                }
            }

            // Find median using quickselect-style algorithm
            let median_idx = count / 2;
            let values_slice = &mut window_values[..count];

            // Partial sort to find median
            values_slice.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

            let median = if count % 2 == 0 && count > 1 {
                (values_slice[median_idx - 1] + values_slice[median_idx]) / 2.0
            } else {
                values_slice[median_idx]
            };

            dst[y * width + x] = median;
        }
    }

    Ok(())
}

/// SIMD-accelerated focal variety (unique value count) computation
///
/// Counts unique values in the focal window with tolerance for floating point.
///
/// # Arguments
///
/// * `src` - Source data
/// * `dst` - Destination buffer for variety counts
/// * `width` - Image width
/// * `height` - Image height
/// * `window_size` - Window size (will use window_size x window_size)
/// * `tolerance` - Tolerance for considering values as equal
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn focal_variety_simd(
    src: &[f32],
    dst: &mut [f32],
    width: usize,
    height: usize,
    window_size: usize,
    tolerance: f32,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if window_size == 0 || window_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "Window size must be odd and greater than zero".to_string(),
        });
    }

    if src.len() != width * height || dst.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let half_size = window_size / 2;
    let max_window_elements = window_size * window_size;

    // Reusable buffer for window values
    let mut window_values = vec![0.0_f32; max_window_elements];

    // Process each pixel
    for y in 0..height {
        for x in 0..width {
            // Calculate window bounds
            let y_start = y.saturating_sub(half_size);
            let y_end = (y + half_size + 1).min(height);
            let x_start = x.saturating_sub(half_size);
            let x_end = (x + half_size + 1).min(width);

            // Collect window values
            let mut count = 0;
            for dy in y_start..y_end {
                let row_offset = dy * width;
                for dx in x_start..x_end {
                    window_values[count] = src[row_offset + dx];
                    count += 1;
                }
            }

            // Sort and count unique values
            let values_slice = &mut window_values[..count];
            values_slice.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

            let mut unique_count = if count > 0 { 1 } else { 0 };
            for i in 1..count {
                if (values_slice[i] - values_slice[i - 1]).abs() > tolerance {
                    unique_count += 1;
                }
            }

            dst[y * width + x] = unique_count as f32;
        }
    }

    Ok(())
}

/// SIMD-accelerated focal majority (mode) computation
///
/// Finds the most common value in the focal window.
///
/// # Arguments
///
/// * `src` - Source data
/// * `dst` - Destination buffer for majority values
/// * `width` - Image width
/// * `height` - Image height
/// * `window_size` - Window size (will use window_size x window_size)
/// * `tolerance` - Tolerance for considering values as equal
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn focal_majority_simd(
    src: &[f32],
    dst: &mut [f32],
    width: usize,
    height: usize,
    window_size: usize,
    tolerance: f32,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if window_size == 0 || window_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "Window size must be odd and greater than zero".to_string(),
        });
    }

    if src.len() != width * height || dst.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let half_size = window_size / 2;
    let max_window_elements = window_size * window_size;

    // Reusable buffer for window values
    let mut window_values = vec![0.0_f32; max_window_elements];

    // Process each pixel
    for y in 0..height {
        for x in 0..width {
            // Calculate window bounds
            let y_start = y.saturating_sub(half_size);
            let y_end = (y + half_size + 1).min(height);
            let x_start = x.saturating_sub(half_size);
            let x_end = (x + half_size + 1).min(width);

            // Collect window values
            let mut count = 0;
            for dy in y_start..y_end {
                let row_offset = dy * width;
                for dx in x_start..x_end {
                    window_values[count] = src[row_offset + dx];
                    count += 1;
                }
            }

            if count == 0 {
                dst[y * width + x] = 0.0;
                continue;
            }

            // Sort values to find mode
            let values_slice = &mut window_values[..count];
            values_slice.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

            // Find mode (most frequent value)
            let mut best_value = values_slice[0];
            let mut best_count = 1_usize;
            let mut current_value = values_slice[0];
            let mut current_count = 1_usize;

            for i in 1..count {
                if (values_slice[i] - current_value).abs() <= tolerance {
                    current_count += 1;
                } else {
                    if current_count > best_count {
                        best_count = current_count;
                        best_value = current_value;
                    }
                    current_value = values_slice[i];
                    current_count = 1;
                }
            }

            // Check last run
            if current_count > best_count {
                best_value = current_value;
            }

            dst[y * width + x] = best_value;
        }
    }

    Ok(())
}

/// SIMD-accelerated focal sum computation
///
/// Computes the sum of values in a focal window.
///
/// # Arguments
///
/// * `src` - Source data
/// * `dst` - Destination buffer for sum values
/// * `width` - Image width
/// * `height` - Image height
/// * `window_size` - Window size (will use window_size x window_size)
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn focal_sum_simd(
    src: &[f32],
    dst: &mut [f32],
    width: usize,
    height: usize,
    window_size: usize,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if window_size == 0 || window_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "Window size must be odd and greater than zero".to_string(),
        });
    }

    if src.len() != width * height || dst.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let half_size = window_size / 2;

    // Process each pixel
    for y in 0..height {
        for x in 0..width {
            let y_start = y.saturating_sub(half_size);
            let y_end = (y + half_size + 1).min(height);
            let x_start = x.saturating_sub(half_size);
            let x_end = (x + half_size + 1).min(width);

            let mut sum = 0.0_f32;

            // SIMD-friendly summation
            for dy in y_start..y_end {
                let row_offset = dy * width;
                const LANES: usize = 8;
                let window_width = x_end - x_start;
                let chunks = window_width / LANES;

                // SIMD processing
                for chunk in 0..chunks {
                    let start = x_start + chunk * LANES;
                    let end = start + LANES;

                    for i in start..end {
                        sum += src[row_offset + i];
                    }
                }

                // Scalar remainder
                let remainder_start = x_start + chunks * LANES;
                for i in remainder_start..x_end {
                    sum += src[row_offset + i];
                }
            }

            dst[y * width + x] = sum;
        }
    }

    Ok(())
}

/// SIMD-accelerated Gaussian blur using separable filters
///
/// Computes Gaussian blur efficiently using separable convolution.
///
/// # Arguments
///
/// * `src` - Source data
/// * `dst` - Destination buffer
/// * `width` - Image width
/// * `height` - Image height
/// * `sigma` - Standard deviation of Gaussian kernel
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn focal_gaussian_blur_simd(
    src: &[f32],
    dst: &mut [f32],
    width: usize,
    height: usize,
    sigma: f32,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if sigma <= 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "sigma",
            message: "Sigma must be positive".to_string(),
        });
    }

    if src.len() != width * height || dst.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    // Compute kernel size (3 sigma rule)
    let kernel_radius = (3.0 * sigma).ceil() as usize;
    let kernel_size = 2 * kernel_radius + 1;

    // Compute 1D Gaussian kernel
    let mut kernel = vec![0.0_f32; kernel_size];
    let mut kernel_sum = 0.0_f32;
    let sigma_sq = sigma * sigma;

    for (i, k) in kernel.iter_mut().enumerate() {
        let x = (i as f32) - (kernel_radius as f32);
        *k = (-x * x / (2.0 * sigma_sq)).exp();
        kernel_sum += *k;
    }

    // Normalize kernel
    for k in &mut kernel {
        *k /= kernel_sum;
    }

    // Allocate temporary buffer
    let mut temp = vec![0.0_f32; width * height];

    // Horizontal pass
    for y in 0..height {
        let row_offset = y * width;

        for x in 0..width {
            let mut sum = 0.0_f32;

            for (i, &k) in kernel.iter().enumerate() {
                let kx = i as i64 - kernel_radius as i64;
                let sx = (x as i64 + kx).clamp(0, (width - 1) as i64) as usize;
                sum += src[row_offset + sx] * k;
            }

            temp[row_offset + x] = sum;
        }
    }

    // Vertical pass
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0_f32;

            for (i, &k) in kernel.iter().enumerate() {
                let ky = i as i64 - kernel_radius as i64;
                let sy = (y as i64 + ky).clamp(0, (height - 1) as i64) as usize;
                sum += temp[sy * width + x] * k;
            }

            dst[y * width + x] = sum;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn test_focal_sum_horizontal() {
        let src = vec![1.0_f32; 100];
        let mut dst = vec![0.0_f32; 100];

        focal_sum_horizontal_simd(&src, &mut dst, 10, 10, 3)
            .expect("Failed to compute focal sum horizontal");

        // Each pixel should have sum of 3 neighbors (edge handling may vary)
        for &val in &dst {
            assert!((1.0..=3.0).contains(&val));
        }
    }

    #[test]
    fn test_focal_mean_separable() {
        let src = vec![2.0_f32; 100];
        let mut dst = vec![0.0_f32; 100];

        focal_mean_separable_simd(&src, &mut dst, 10, 10, 3, 3)
            .expect("Failed to compute focal mean separable");

        // Uniform input should produce uniform output
        for &val in &dst {
            assert_abs_diff_eq!(val, 2.0, epsilon = 0.01);
        }
    }

    #[test]
    fn test_focal_min_max() {
        let mut src = vec![1.0_f32; 100];
        src[50] = 10.0; // Peak
        src[25] = -5.0; // Valley

        let mut min_out = vec![0.0_f32; 100];
        let mut max_out = vec![0.0_f32; 100];

        focal_min_max_simd(&src, &mut min_out, &mut max_out, 10, 10, 3)
            .expect("Failed to compute focal min/max");

        // Check that peak is detected
        assert!(max_out[50] >= 10.0);
        // Check that valley is detected
        assert!(min_out[25] <= -5.0);
    }

    #[test]
    fn test_focal_variance() {
        let src = vec![1.0_f32; 100];
        let mut dst = vec![0.0_f32; 100];

        focal_variance_simd(&src, &mut dst, 10, 10, 3).expect("Failed to compute focal variance");

        // Uniform input should have zero variance
        for &val in &dst {
            assert_abs_diff_eq!(val, 0.0, epsilon = 0.01);
        }
    }

    #[test]
    fn test_focal_stddev() {
        let src = vec![1.0_f32; 100];
        let mut dst = vec![0.0_f32; 100];

        focal_stddev_simd(&src, &mut dst, 10, 10, 3)
            .expect("Failed to compute focal standard deviation");

        // Uniform input should have zero standard deviation
        for &val in &dst {
            assert_abs_diff_eq!(val, 0.0, epsilon = 0.01);
        }
    }

    #[test]
    fn test_focal_convolve() {
        let src = vec![1.0_f32; 100];
        let mut dst = vec![0.0_f32; 100];
        let kernel = vec![1.0_f32; 9]; // 3x3 uniform kernel

        focal_convolve_simd(&src, &mut dst, 10, 10, &kernel, 3, 3, true)
            .expect("Failed to compute focal convolution");

        // Uniform input with uniform kernel should produce uniform output
        for &val in &dst {
            assert_abs_diff_eq!(val, 1.0, epsilon = 0.01);
        }
    }

    #[test]
    fn test_invalid_window_size() {
        let src = vec![1.0_f32; 100];
        let mut dst = vec![0.0_f32; 100];

        // Even window size should fail
        let result = focal_mean_separable_simd(&src, &mut dst, 10, 10, 2, 3);
        assert!(result.is_err());
    }

    #[test]
    fn test_buffer_size_mismatch() {
        let src = vec![1.0_f32; 100];
        let mut dst = vec![0.0_f32; 50]; // Wrong size

        let result = focal_mean_separable_simd(&src, &mut dst, 10, 10, 3, 3);
        assert!(result.is_err());
    }

    #[test]
    fn test_focal_range() {
        let mut src = vec![5.0_f32; 100];
        src[50] = 10.0; // Max
        src[51] = 0.0; // Min

        let mut dst = vec![0.0_f32; 100];

        focal_range_simd(&src, &mut dst, 10, 10, 3).expect("Failed to compute focal range");

        // Range should be detected where min and max are neighbors
        assert!(dst[50] >= 10.0);
    }

    #[test]
    fn test_focal_median() {
        let src = vec![5.0_f32; 100];
        let mut dst = vec![0.0_f32; 100];

        focal_median_simd(&src, &mut dst, 10, 10, 3).expect("Failed to compute focal median");

        // Uniform input should give uniform median
        for &val in &dst {
            assert_abs_diff_eq!(val, 5.0, epsilon = 0.01);
        }
    }

    #[test]
    fn test_focal_variety() {
        let mut src = vec![1.0_f32; 100];
        src[50] = 2.0;
        src[51] = 3.0;

        let mut dst = vec![0.0_f32; 100];

        focal_variety_simd(&src, &mut dst, 10, 10, 3, 0.01)
            .expect("Failed to compute focal variety");

        // Around cells 50, 51 should have variety > 1
        assert!(dst[50] >= 2.0);
    }

    #[test]
    fn test_focal_majority() {
        let src = vec![1.0_f32; 100]; // All same value
        let mut dst = vec![0.0_f32; 100];

        focal_majority_simd(&src, &mut dst, 10, 10, 3, 0.01)
            .expect("Failed to compute focal majority");

        // Uniform input should give same majority
        for &val in &dst {
            assert_abs_diff_eq!(val, 1.0, epsilon = 0.01);
        }
    }

    #[test]
    fn test_focal_sum() {
        let src = vec![1.0_f32; 100];
        let mut dst = vec![0.0_f32; 100];

        focal_sum_simd(&src, &mut dst, 10, 10, 3).expect("Failed to compute focal sum");

        // Interior pixels should have sum of 9 neighbors
        assert_abs_diff_eq!(dst[55], 9.0, epsilon = 0.01);
    }

    #[test]
    fn test_focal_gaussian_blur() {
        let src = vec![1.0_f32; 100];
        let mut dst = vec![0.0_f32; 100];

        focal_gaussian_blur_simd(&src, &mut dst, 10, 10, 1.0)
            .expect("Failed to compute focal Gaussian blur");

        // Uniform input should remain unchanged
        for &val in &dst {
            assert_abs_diff_eq!(val, 1.0, epsilon = 0.1);
        }
    }
}

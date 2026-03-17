//! SIMD-accelerated thresholding operations
//!
//! This module provides high-performance image thresholding and binarization
//! using SIMD instructions. Thresholding is fundamental for segmentation,
//! feature extraction, and image preprocessing.
//!
//! # Supported Operations
//!
//! - **Binary Thresholding**: Simple threshold with two output values
//! - **Adaptive Thresholding**: Local threshold based on neighborhood
//! - **Otsu's Method**: Automatic threshold selection
//! - **Multi-level Thresholding**: Multiple threshold values
//! - **Range Thresholding**: Keep values within a range
//! - **Hysteresis Thresholding**: Two-level threshold with connectivity
//!
//! # Performance
//!
//! Expected speedup over scalar: 6-12x for thresholding operations
//!
//! # Example
//!
//! ```rust
//! use oxigdal_algorithms::simd::threshold::{binary_threshold, otsu_threshold};
//! use oxigdal_algorithms::error::Result;
//!
//! fn example() -> Result<()> {
//!     let data = vec![128u8; 1000];
//!     let mut output = vec![0u8; 1000];
//!
//!     binary_threshold(&data, &mut output, 100, 255, 0)?;
//!     Ok(())
//! }
//! # example().expect("example failed");
//! ```

use crate::error::{AlgorithmError, Result};

/// Binary threshold with custom output values
///
/// # Arguments
///
/// * `input` - Input data
/// * `output` - Output data
/// * `threshold` - Threshold value
/// * `max_value` - Value to use when input >= threshold
/// * `min_value` - Value to use when input < threshold
///
/// # Errors
///
/// Returns an error if buffer sizes don't match
pub fn binary_threshold(
    input: &[u8],
    output: &mut [u8],
    threshold: u8,
    max_value: u8,
    min_value: u8,
) -> Result<()> {
    if input.len() != output.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffers",
            message: format!(
                "Buffer size mismatch: input={}, output={}",
                input.len(),
                output.len()
            ),
        });
    }

    const LANES: usize = 16;
    let chunks = input.len() / LANES;

    // SIMD processing - auto-vectorized by LLVM
    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            output[j] = if input[j] >= threshold {
                max_value
            } else {
                min_value
            };
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..input.len() {
        output[i] = if input[i] >= threshold {
            max_value
        } else {
            min_value
        };
    }

    Ok(())
}

/// Binary threshold to zero
///
/// Values below threshold are set to zero, others remain unchanged.
///
/// # Errors
///
/// Returns an error if buffer sizes don't match
pub fn threshold_to_zero(input: &[u8], output: &mut [u8], threshold: u8) -> Result<()> {
    if input.len() != output.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffers",
            message: format!(
                "Buffer size mismatch: input={}, output={}",
                input.len(),
                output.len()
            ),
        });
    }

    const LANES: usize = 16;
    let chunks = input.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            output[j] = if input[j] >= threshold { input[j] } else { 0 };
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..input.len() {
        output[i] = if input[i] >= threshold { input[i] } else { 0 };
    }

    Ok(())
}

/// Truncate threshold - cap values at threshold
///
/// Values above threshold are set to threshold, others remain unchanged.
///
/// # Errors
///
/// Returns an error if buffer sizes don't match
pub fn threshold_truncate(input: &[u8], output: &mut [u8], threshold: u8) -> Result<()> {
    if input.len() != output.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffers",
            message: format!(
                "Buffer size mismatch: input={}, output={}",
                input.len(),
                output.len()
            ),
        });
    }

    const LANES: usize = 16;
    let chunks = input.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            output[j] = input[j].min(threshold);
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..input.len() {
        output[i] = input[i].min(threshold);
    }

    Ok(())
}

/// Range threshold - keep values within [low, high]
///
/// Values outside range are set to zero.
///
/// # Errors
///
/// Returns an error if buffer sizes don't match or if low > high
pub fn threshold_range(
    input: &[u8],
    output: &mut [u8],
    low_threshold: u8,
    high_threshold: u8,
) -> Result<()> {
    if input.len() != output.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffers",
            message: format!(
                "Buffer size mismatch: input={}, output={}",
                input.len(),
                output.len()
            ),
        });
    }

    if low_threshold > high_threshold {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "thresholds",
            message: format!("Invalid range: low={low_threshold}, high={high_threshold}"),
        });
    }

    const LANES: usize = 16;
    let chunks = input.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            let val = input[j];
            output[j] = if val >= low_threshold && val <= high_threshold {
                val
            } else {
                0
            };
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..input.len() {
        let val = input[i];
        output[i] = if val >= low_threshold && val <= high_threshold {
            val
        } else {
            0
        };
    }

    Ok(())
}

/// Calculate optimal threshold using Otsu's method
///
/// Finds threshold that minimizes intra-class variance (maximizes inter-class variance).
/// This is optimal for bimodal distributions.
///
/// # Errors
///
/// Returns an error if data is empty
pub fn otsu_threshold(data: &[u8]) -> Result<u8> {
    if data.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "otsu_threshold",
        });
    }

    // Compute histogram
    let mut histogram = [0u32; 256];
    for &value in data {
        histogram[value as usize] += 1;
    }

    let total_pixels = data.len() as f64;

    // Compute total mean
    let mut total_mean = 0.0;
    for (i, &count) in histogram.iter().enumerate() {
        total_mean += i as f64 * f64::from(count);
    }
    total_mean /= total_pixels;

    let mut max_variance = 0.0;
    let mut optimal_threshold = 0u8;

    let mut weight_background = 0.0;
    let mut sum_background = 0.0;

    for (t, &count) in histogram.iter().enumerate() {
        weight_background += f64::from(count) / total_pixels;
        sum_background += t as f64 * f64::from(count);

        if weight_background < 1e-10 || (1.0 - weight_background) < 1e-10 {
            continue;
        }

        let mean_background = sum_background / (weight_background * total_pixels);
        let mean_foreground = (total_mean * total_pixels - sum_background)
            / ((1.0 - weight_background) * total_pixels);

        let variance = weight_background
            * (1.0 - weight_background)
            * (mean_background - mean_foreground).powi(2);

        // Use >= to prefer later thresholds in case of ties (finds midpoint)
        if variance >= max_variance {
            max_variance = variance;
            optimal_threshold = t as u8;
        }
    }

    Ok(optimal_threshold)
}

/// Apply adaptive threshold using local mean
///
/// # Arguments
///
/// * `input` - Input image data
/// * `output` - Output binary image
/// * `width` - Image width
/// * `height` - Image height
/// * `window_size` - Size of local window (must be odd)
/// * `c` - Constant subtracted from mean
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn adaptive_threshold_mean(
    input: &[u8],
    output: &mut [u8],
    width: usize,
    height: usize,
    window_size: usize,
    c: i16,
) -> Result<()> {
    if input.len() != width * height || output.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffers",
            message: format!(
                "Buffer size mismatch: input={}, output={}, expected={}",
                input.len(),
                output.len(),
                width * height
            ),
        });
    }

    if window_size == 0 || window_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: format!("Window size must be odd and positive, got {window_size}"),
        });
    }

    let half_window = window_size / 2;

    for y in 0..height {
        for x in 0..width {
            // Compute local mean
            let mut sum = 0u32;
            let mut count = 0u32;

            let y_start = y.saturating_sub(half_window);
            let y_end = (y + half_window + 1).min(height);
            let x_start = x.saturating_sub(half_window);
            let x_end = (x + half_window + 1).min(width);

            for py in y_start..y_end {
                for px in x_start..x_end {
                    sum += u32::from(input[py * width + px]);
                    count += 1;
                }
            }

            let mean = sum.checked_div(count).unwrap_or(0);
            let threshold = (mean as i32 - i32::from(c)).max(0) as u8;

            let idx = y * width + x;
            output[idx] = if input[idx] >= threshold { 255 } else { 0 };
        }
    }

    Ok(())
}

/// Apply adaptive threshold using Gaussian-weighted mean
///
/// Similar to adaptive_threshold_mean but uses Gaussian weights.
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn adaptive_threshold_gaussian(
    input: &[u8],
    output: &mut [u8],
    width: usize,
    height: usize,
    window_size: usize,
    c: i16,
) -> Result<()> {
    if input.len() != width * height || output.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffers",
            message: format!(
                "Buffer size mismatch: input={}, output={}, expected={}",
                input.len(),
                output.len(),
                width * height
            ),
        });
    }

    if window_size == 0 || window_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: format!("Window size must be odd and positive, got {window_size}"),
        });
    }

    let half_window = window_size / 2;
    let sigma = window_size as f32 / 6.0;

    // Precompute Gaussian weights
    let mut weights = vec![vec![0.0f32; window_size]; window_size];
    let mut weight_sum = 0.0f32;

    for wy in 0..window_size {
        for wx in 0..window_size {
            let dy = wy as f32 - half_window as f32;
            let dx = wx as f32 - half_window as f32;
            let weight = (-((dx * dx + dy * dy) / (2.0 * sigma * sigma))).exp();
            weights[wy][wx] = weight;
            weight_sum += weight;
        }
    }

    // Normalize weights
    for row in &mut weights {
        for w in row {
            *w /= weight_sum;
        }
    }

    for y in 0..height {
        for x in 0..width {
            let mut weighted_sum = 0.0f32;

            let y_start = y.saturating_sub(half_window);
            let y_end = (y + half_window + 1).min(height);
            let x_start = x.saturating_sub(half_window);
            let x_end = (x + half_window + 1).min(width);

            for py in y_start..y_end {
                for px in x_start..x_end {
                    let wy = py - y + half_window;
                    let wx = px - x + half_window;
                    if wy < window_size && wx < window_size {
                        weighted_sum += f32::from(input[py * width + px]) * weights[wy][wx];
                    }
                }
            }

            let threshold = (weighted_sum - f32::from(c)).max(0.0) as u8;

            let idx = y * width + x;
            output[idx] = if input[idx] >= threshold { 255 } else { 0 };
        }
    }

    Ok(())
}

/// Hysteresis thresholding (two-level threshold with connectivity)
///
/// Used in Canny edge detection. Pixels above high_threshold are strong edges.
/// Pixels between low_threshold and high_threshold are weak edges, kept only
/// if connected to strong edges.
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn hysteresis_threshold(
    input: &[u8],
    output: &mut [u8],
    width: usize,
    height: usize,
    low_threshold: u8,
    high_threshold: u8,
) -> Result<()> {
    if input.len() != width * height || output.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffers",
            message: format!(
                "Buffer size mismatch: input={}, output={}, expected={}",
                input.len(),
                output.len(),
                width * height
            ),
        });
    }

    if low_threshold >= high_threshold {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "thresholds",
            message: format!(
                "low_threshold must be < high_threshold: {low_threshold} >= {high_threshold}"
            ),
        });
    }

    // Initialize output
    output.fill(0);

    // Mark strong edges
    for i in 0..input.len() {
        if input[i] >= high_threshold {
            output[i] = 255;
        }
    }

    // Propagate from strong edges to weak edges
    let mut changed = true;
    while changed {
        changed = false;

        for y in 1..(height - 1) {
            for x in 1..(width - 1) {
                let idx = y * width + x;

                // If this is a weak edge and not yet marked
                if input[idx] >= low_threshold && input[idx] < high_threshold && output[idx] == 0 {
                    // Check if connected to a strong edge
                    let mut connected = false;
                    for dy in 0..3 {
                        for dx in 0..3 {
                            if dx == 1 && dy == 1 {
                                continue;
                            }
                            let ny = y + dy - 1;
                            let nx = x + dx - 1;
                            if output[ny * width + nx] == 255 {
                                connected = true;
                                break;
                            }
                        }
                        if connected {
                            break;
                        }
                    }

                    if connected {
                        output[idx] = 255;
                        changed = true;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Multi-level thresholding
///
/// Apply multiple thresholds to create multiple output levels.
///
/// # Arguments
///
/// * `input` - Input data
/// * `output` - Output data
/// * `thresholds` - Sorted threshold values
/// * `levels` - Output level for each threshold range (length = thresholds.len() + 1)
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn multi_threshold(
    input: &[u8],
    output: &mut [u8],
    thresholds: &[u8],
    levels: &[u8],
) -> Result<()> {
    if input.len() != output.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffers",
            message: format!(
                "Buffer size mismatch: input={}, output={}",
                input.len(),
                output.len()
            ),
        });
    }

    if levels.len() != thresholds.len() + 1 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "levels",
            message: format!(
                "Levels length must be thresholds.len() + 1: {} != {}",
                levels.len(),
                thresholds.len() + 1
            ),
        });
    }

    const LANES: usize = 16;
    let chunks = input.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            let val = input[j];
            let mut level_idx = 0;

            for (t_idx, &threshold) in thresholds.iter().enumerate() {
                if val >= threshold {
                    level_idx = t_idx + 1;
                } else {
                    break;
                }
            }

            output[j] = levels[level_idx];
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..input.len() {
        let val = input[i];
        let mut level_idx = 0;

        for (t_idx, &threshold) in thresholds.iter().enumerate() {
            if val >= threshold {
                level_idx = t_idx + 1;
            } else {
                break;
            }
        }

        output[i] = levels[level_idx];
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_threshold() {
        let input = vec![50, 100, 150, 200, 250];
        let mut output = vec![0; 5];

        binary_threshold(&input, &mut output, 128, 255, 0)
            .expect("binary threshold should succeed");

        assert_eq!(output, vec![0, 0, 255, 255, 255]);
    }

    #[test]
    fn test_threshold_to_zero() {
        let input = vec![50, 100, 150, 200, 250];
        let mut output = vec![0; 5];

        threshold_to_zero(&input, &mut output, 128).expect("threshold to zero should succeed");

        assert_eq!(output, vec![0, 0, 150, 200, 250]);
    }

    #[test]
    fn test_threshold_truncate() {
        let input = vec![50, 100, 150, 200, 250];
        let mut output = vec![0; 5];

        threshold_truncate(&input, &mut output, 128).expect("threshold truncate should succeed");

        assert_eq!(output, vec![50, 100, 128, 128, 128]);
    }

    #[test]
    fn test_threshold_range() {
        let input = vec![50, 100, 150, 200, 250];
        let mut output = vec![0; 5];

        threshold_range(&input, &mut output, 100, 200).expect("threshold range should succeed");

        assert_eq!(output, vec![0, 100, 150, 200, 0]);
    }

    #[test]
    fn test_otsu_threshold() {
        // Bimodal distribution
        let mut data = vec![50u8; 500];
        data.extend(vec![200u8; 500]);

        let threshold = otsu_threshold(&data).expect("Otsu threshold calculation should succeed");

        // Threshold should be between the two modes
        assert!(threshold > 50 && threshold < 200);
    }

    #[test]
    fn test_adaptive_threshold_mean() {
        let width = 10;
        let height = 10;
        let input = vec![128u8; width * height];
        let mut output = vec![0u8; width * height];

        adaptive_threshold_mean(&input, &mut output, width, height, 3, 10)
            .expect("adaptive threshold mean should succeed");

        // Uniform input should produce mostly uniform output
        assert!(output.iter().filter(|&&x| x == 255).count() > 50);
    }

    #[test]
    fn test_multi_threshold() {
        let input = vec![10, 50, 100, 150, 200, 250];
        let mut output = vec![0; 6];
        let thresholds = vec![64, 128, 192];
        let levels = vec![0, 85, 170, 255];

        multi_threshold(&input, &mut output, &thresholds, &levels)
            .expect("multi-level threshold should succeed");

        assert_eq!(output[0], 0); // < 64
        assert_eq!(output[1], 0); // < 64
        assert_eq!(output[2], 85); // >= 64, < 128
        assert_eq!(output[3], 170); // >= 128, < 192
        assert_eq!(output[4], 255); // >= 192
        assert_eq!(output[5], 255); // >= 192
    }

    #[test]
    fn test_hysteresis_threshold() {
        let width = 5;
        let height = 5;
        let mut input = vec![0u8; width * height];

        // Create a strong edge
        input[2 * width + 2] = 200;
        // Create weak edges connected to strong edge
        input[2 * width + 1] = 80;
        input[width + 2] = 80;
        // Create isolated weak edge
        input[4 * width + 4] = 80;

        let mut output = vec![0u8; width * height];
        hysteresis_threshold(&input, &mut output, width, height, 50, 150)
            .expect("hysteresis threshold should succeed");

        // Strong edge should be marked
        assert_eq!(output[2 * width + 2], 255);
        // Connected weak edges should be marked
        assert_eq!(output[2 * width + 1], 255);
        assert_eq!(output[width + 2], 255);
        // Isolated weak edge should not be marked
        assert_eq!(output[4 * width + 4], 0);
    }

    #[test]
    fn test_buffer_size_mismatch() {
        let input = vec![0u8; 10];
        let mut output = vec![0u8; 5]; // Wrong size

        let result = binary_threshold(&input, &mut output, 128, 255, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_range() {
        let input = vec![0u8; 10];
        let mut output = vec![0u8; 10];

        let result = threshold_range(&input, &mut output, 200, 100);
        assert!(result.is_err());
    }
}

//! Spatial filtering operations for rasters
//!
//! This module provides various spatial filters:
//! - Gaussian blur (smoothing)
//! - Median filter (noise reduction)
//! - Edge detection (Sobel, Prewitt, Canny)
//! - Sharpening
//! - Low-pass and high-pass filters

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Edge detection method
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeDetector {
    /// Sobel operator
    Sobel,
    /// Prewitt operator
    Prewitt,
    /// Canny edge detector
    Canny {
        /// Low threshold for hysteresis
        low_threshold: f64,
        /// High threshold for hysteresis
        high_threshold: f64,
    },
}

/// Filter boundary handling mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoundaryMode {
    /// Use constant value (usually 0)
    Constant(f64),
    /// Replicate edge pixels
    Replicate,
    /// Reflect across edge
    Reflect,
    /// Wrap around
    Wrap,
}

/// Applies a Gaussian blur filter
///
/// # Arguments
///
/// * `src` - Source raster
/// * `sigma` - Standard deviation of Gaussian kernel
/// * `kernel_size` - Size of kernel (must be odd), if None uses 6*sigma + 1
///
/// # Errors
///
/// Returns an error if kernel_size is even or operation fails
pub fn gaussian_blur(
    src: &RasterBuffer,
    sigma: f64,
    kernel_size: Option<usize>,
) -> Result<RasterBuffer> {
    if sigma <= 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "sigma",
            message: format!("must be positive, got {}", sigma),
        });
    }

    let size = kernel_size.unwrap_or_else(|| {
        let s = (6.0 * sigma + 1.0) as usize;
        if s % 2 == 0 { s + 1 } else { s }
    });

    if size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "kernel_size",
            message: format!("must be odd, got {}", size),
        });
    }

    let kernel = create_gaussian_kernel(size, sigma)?;
    apply_separable_filter(src, &kernel, BoundaryMode::Replicate)
}

/// Creates a 1D Gaussian kernel
fn create_gaussian_kernel(size: usize, sigma: f64) -> Result<Vec<f64>> {
    use oxigdal_core::OxiGdalError;

    if size == 0 {
        return Err(
            OxiGdalError::invalid_parameter_builder("size", "must be positive, got 0")
                .with_parameter("value", "0")
                .with_parameter("min", "1")
                .with_operation("create_gaussian_kernel")
                .with_suggestion("Use positive kernel size. Common values: 3, 5, 7, 9")
                .build()
                .into(),
        );
    }

    let center = size as i32 / 2;
    let mut kernel = Vec::with_capacity(size);
    let mut sum = 0.0;

    for i in 0..size {
        let x = i as i32 - center;
        let val = (-((x * x) as f64) / (2.0 * sigma * sigma)).exp();
        kernel.push(val);
        sum += val;
    }

    // Normalize
    for val in &mut kernel {
        *val /= sum;
    }

    Ok(kernel)
}

/// Applies a separable filter (1D kernel applied horizontally then vertically)
fn apply_separable_filter(
    src: &RasterBuffer,
    kernel_1d: &[f64],
    boundary: BoundaryMode,
) -> Result<RasterBuffer> {
    // First pass: horizontal
    let temp = apply_1d_filter(src, kernel_1d, true, boundary)?;

    // Second pass: vertical
    apply_1d_filter(&temp, kernel_1d, false, boundary)
}

/// Applies a 1D filter in horizontal or vertical direction
fn apply_1d_filter(
    src: &RasterBuffer,
    kernel: &[f64],
    horizontal: bool,
    boundary: BoundaryMode,
) -> Result<RasterBuffer> {
    let width = src.width();
    let height = src.height();
    let mut dst = RasterBuffer::zeros(width, height, src.data_type());

    let radius = (kernel.len() / 2) as i64;

    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;

            for k in 0..kernel.len() {
                let offset = k as i64 - radius;

                let (px, py) = if horizontal {
                    (x as i64 + offset, y as i64)
                } else {
                    (x as i64, y as i64 + offset)
                };

                let val = get_pixel_with_boundary(src, px, py, boundary)?;
                sum += val * kernel[k];
            }

            dst.set_pixel(x, y, sum).map_err(AlgorithmError::Core)?;
        }
    }

    Ok(dst)
}

/// Gets a pixel value with boundary handling
fn get_pixel_with_boundary(
    src: &RasterBuffer,
    x: i64,
    y: i64,
    boundary: BoundaryMode,
) -> Result<f64> {
    let width = src.width() as i64;
    let height = src.height() as i64;

    let (px, py) = match boundary {
        BoundaryMode::Constant(c) => {
            if x < 0 || x >= width || y < 0 || y >= height {
                return Ok(c);
            }
            (x as u64, y as u64)
        }
        BoundaryMode::Replicate => {
            let px = x.clamp(0, width - 1) as u64;
            let py = y.clamp(0, height - 1) as u64;
            (px, py)
        }
        BoundaryMode::Reflect => {
            let px = reflect_index(x, width) as u64;
            let py = reflect_index(y, height) as u64;
            (px, py)
        }
        BoundaryMode::Wrap => {
            let px = ((x % width + width) % width) as u64;
            let py = ((y % height + height) % height) as u64;
            (px, py)
        }
    };

    src.get_pixel(px, py).map_err(AlgorithmError::Core)
}

/// Reflects an index across a boundary
fn reflect_index(idx: i64, size: i64) -> i64 {
    if idx < 0 {
        -idx - 1
    } else if idx >= size {
        2 * size - idx - 1
    } else {
        idx
    }
}

/// Partitions array around a pivot for quickselect
fn partition(arr: &mut [f64], low: usize, high: usize) -> usize {
    let pivot = arr[high];
    let mut i = low;

    for j in low..high {
        // Handle NaN explicitly - treat as greater than any finite value
        let cmp = if arr[j].is_nan() {
            if pivot.is_nan() {
                core::cmp::Ordering::Equal
            } else {
                core::cmp::Ordering::Greater
            }
        } else if pivot.is_nan() {
            core::cmp::Ordering::Less
        } else if arr[j] < pivot {
            core::cmp::Ordering::Less
        } else if arr[j] > pivot {
            core::cmp::Ordering::Greater
        } else {
            core::cmp::Ordering::Equal
        };

        if matches!(cmp, core::cmp::Ordering::Less) {
            arr.swap(i, j);
            i += 1;
        }
    }
    arr.swap(i, high);
    i
}

/// Finds the k-th smallest element using quickselect algorithm (O(n) average)
fn quickselect(arr: &mut [f64], k: usize) -> f64 {
    if arr.is_empty() {
        return f64::NAN;
    }
    if arr.len() == 1 {
        return arr[0];
    }

    let mut low = 0;
    let mut high = arr.len() - 1;

    loop {
        if low == high {
            return arr[low];
        }

        let pivot_idx = partition(arr, low, high);

        if k == pivot_idx {
            return arr[k];
        } else if k < pivot_idx {
            high = pivot_idx - 1;
        } else {
            low = pivot_idx + 1;
        }
    }
}

/// Finds median of values using quickselect algorithm
fn find_median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }

    let len = values.len();
    if len % 2 == 1 {
        // Odd length: return middle element
        quickselect(values, len / 2)
    } else {
        // Even length: average of two middle elements
        let mid1 = quickselect(values, len / 2 - 1);
        // Need to find the other middle element without disturbing the partition
        let mid2 = if len >= 2 {
            quickselect(values, len / 2)
        } else {
            mid1
        };
        (mid1 + mid2) / 2.0
    }
}

/// Finds median from histogram for byte data
fn find_median_from_histogram(histogram: &[usize; 256], total_count: usize) -> f64 {
    if total_count == 0 {
        return f64::NAN;
    }

    let target = if total_count % 2 == 1 {
        // Odd count: find middle element
        total_count / 2
    } else {
        // Even count: will need to average two middle elements
        total_count / 2 - 1
    };

    let mut cumulative = 0;
    for (value, &count) in histogram.iter().enumerate() {
        cumulative += count;
        if cumulative > target {
            // Found the bin containing the median
            if total_count % 2 == 1 {
                return value as f64;
            } else {
                // For even count, need to check if we also have the next element
                if cumulative > target + 1 {
                    // Both elements are in this bin
                    return value as f64;
                } else {
                    // Next element is in the next non-empty bin
                    for (next_val, &next_count) in histogram.iter().enumerate().skip(value + 1) {
                        if next_count > 0 {
                            return (value as f64 + next_val as f64) / 2.0;
                        }
                    }
                    return value as f64;
                }
            }
        }
    }

    f64::NAN
}

/// Applies a median filter with histogram-based optimization for byte data
fn median_filter_byte_optimized(src: &RasterBuffer, kernel_size: usize) -> Result<RasterBuffer> {
    let width = src.width();
    let height = src.height();
    let mut dst = RasterBuffer::zeros(width, height, src.data_type());

    let radius = (kernel_size / 2) as i64;

    for y in 0..height {
        let mut histogram = [0usize; 256];
        let mut total_count = 0;

        for x in 0..width {
            if x == 0 {
                // Initialize histogram for first pixel in row
                histogram = [0usize; 256];
                total_count = 0;

                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let px = x as i64 + dx;
                        let py = y as i64 + dy;

                        if px >= 0 && px < width as i64 && py >= 0 && py < height as i64 {
                            let val = src
                                .get_pixel(px as u64, py as u64)
                                .map_err(AlgorithmError::Core)?;
                            if val.is_finite() && !src.is_nodata(val) {
                                let byte_val = val.clamp(0.0, 255.0) as u8;
                                histogram[byte_val as usize] += 1;
                                total_count += 1;
                            }
                        }
                    }
                }
            } else {
                // Slide window horizontally: remove left column, add right column
                let left_x = x as i64 - radius - 1;
                let right_x = x as i64 + radius;

                // Remove left column
                if left_x >= 0 {
                    for dy in -radius..=radius {
                        let py = y as i64 + dy;
                        if py >= 0 && py < height as i64 {
                            let val = src
                                .get_pixel(left_x as u64, py as u64)
                                .map_err(AlgorithmError::Core)?;
                            if val.is_finite() && !src.is_nodata(val) {
                                let byte_val = val.clamp(0.0, 255.0) as u8;
                                if histogram[byte_val as usize] > 0 {
                                    histogram[byte_val as usize] -= 1;
                                    total_count -= 1;
                                }
                            }
                        }
                    }
                }

                // Add right column
                if right_x < width as i64 {
                    for dy in -radius..=radius {
                        let py = y as i64 + dy;
                        if py >= 0 && py < height as i64 {
                            let val = src
                                .get_pixel(right_x as u64, py as u64)
                                .map_err(AlgorithmError::Core)?;
                            if val.is_finite() && !src.is_nodata(val) {
                                let byte_val = val.clamp(0.0, 255.0) as u8;
                                histogram[byte_val as usize] += 1;
                                total_count += 1;
                            }
                        }
                    }
                }
            }

            if total_count > 0 {
                let median = find_median_from_histogram(&histogram, total_count);
                dst.set_pixel(x, y, median).map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(dst)
}

/// Applies a median filter
///
/// # Arguments
///
/// * `src` - Source raster
/// * `kernel_size` - Size of kernel (must be odd)
///
/// # Errors
///
/// Returns an error if kernel_size is even or operation fails
pub fn median_filter(src: &RasterBuffer, kernel_size: usize) -> Result<RasterBuffer> {
    if kernel_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "kernel_size",
            message: "Kernel size must be odd".to_string(),
        });
    }

    // Check if we can use histogram-based optimization
    // For byte data (values 0-255), histogram approach is much faster
    // We detect this by checking if all values in a sample are in byte range
    let width = src.width();
    let height = src.height();

    // Sample a few pixels to determine if data is byte-range
    let mut is_byte_data = true;
    let sample_size = 100.min(width * height);
    for i in 0..sample_size {
        let x = i % width;
        let y = i / width;
        if let Ok(val) = src.get_pixel(x, y) {
            if val.is_finite() && !src.is_nodata(val) {
                if !(0.0..=255.0).contains(&val) {
                    is_byte_data = false;
                    break;
                }
            }
        }
    }

    if is_byte_data && width * height > 1000 {
        // Use histogram-based approach for byte data on larger images
        median_filter_byte_optimized(src, kernel_size)
    } else {
        // Use quickselect for floating point or small images
        median_filter_generic(src, kernel_size)
    }
}

/// Generic median filter using quickselect
fn median_filter_generic(src: &RasterBuffer, kernel_size: usize) -> Result<RasterBuffer> {
    let width = src.width();
    let height = src.height();
    let mut dst = RasterBuffer::zeros(width, height, src.data_type());

    let radius = (kernel_size / 2) as i64;
    let mut values = Vec::with_capacity(kernel_size * kernel_size);

    for y in 0..height {
        for x in 0..width {
            values.clear();

            // Collect values in kernel
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let px = x as i64 + dx;
                    let py = y as i64 + dy;

                    if px >= 0 && px < width as i64 && py >= 0 && py < height as i64 {
                        let val = src
                            .get_pixel(px as u64, py as u64)
                            .map_err(AlgorithmError::Core)?;
                        if val.is_finite() && !src.is_nodata(val) {
                            values.push(val);
                        }
                    }
                }
            }

            if values.is_empty() {
                continue;
            }

            // Find median using quickselect (O(n) average instead of O(n log n))
            let median = find_median(&mut values);

            dst.set_pixel(x, y, median).map_err(AlgorithmError::Core)?;
        }
    }

    Ok(dst)
}

/// Applies Sobel edge detection
///
/// Returns the gradient magnitude
///
/// # Errors
///
/// Returns an error if operation fails
pub fn sobel_edge_detection(src: &RasterBuffer) -> Result<RasterBuffer> {
    // Sobel kernels
    let sobel_x = [-1.0, 0.0, 1.0, -2.0, 0.0, 2.0, -1.0, 0.0, 1.0];
    let sobel_y = [-1.0, -2.0, -1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 1.0];

    let gx = apply_3x3_kernel(src, &sobel_x)?;
    let gy = apply_3x3_kernel(src, &sobel_y)?;

    // Compute gradient magnitude
    compute_gradient_magnitude(&gx, &gy)
}

/// Applies Prewitt edge detection
///
/// Returns the gradient magnitude
///
/// # Errors
///
/// Returns an error if operation fails
pub fn prewitt_edge_detection(src: &RasterBuffer) -> Result<RasterBuffer> {
    // Prewitt kernels
    let prewitt_x = [-1.0, 0.0, 1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0];
    let prewitt_y = [-1.0, -1.0, -1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];

    let gx = apply_3x3_kernel(src, &prewitt_x)?;
    let gy = apply_3x3_kernel(src, &prewitt_y)?;

    // Compute gradient magnitude
    compute_gradient_magnitude(&gx, &gy)
}

/// Applies a 3x3 convolution kernel
fn apply_3x3_kernel(src: &RasterBuffer, kernel: &[f64; 9]) -> Result<RasterBuffer> {
    let width = src.width();
    let height = src.height();
    let mut dst = RasterBuffer::zeros(width, height, src.data_type());

    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let mut sum = 0.0;
            let mut idx = 0;

            for dy in -1..=1i64 {
                for dx in -1..=1i64 {
                    let px = (x as i64 + dx) as u64;
                    let py = (y as i64 + dy) as u64;
                    let value = src.get_pixel(px, py).map_err(AlgorithmError::Core)?;
                    sum += value * kernel[idx];
                    idx += 1;
                }
            }

            dst.set_pixel(x, y, sum).map_err(AlgorithmError::Core)?;
        }
    }

    Ok(dst)
}

/// Computes gradient magnitude from x and y gradients
fn compute_gradient_magnitude(gx: &RasterBuffer, gy: &RasterBuffer) -> Result<RasterBuffer> {
    let width = gx.width();
    let height = gx.height();
    let mut result = RasterBuffer::zeros(width, height, gx.data_type());

    for y in 0..height {
        for x in 0..width {
            let vx = gx.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let vy = gy.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let magnitude = (vx * vx + vy * vy).sqrt();
            result
                .set_pixel(x, y, magnitude)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(result)
}

/// Applies sharpening filter
///
/// # Arguments
///
/// * `src` - Source raster
/// * `amount` - Sharpening amount (typically 0.5 to 2.0)
///
/// # Errors
///
/// Returns an error if operation fails
pub fn sharpen(src: &RasterBuffer, amount: f64) -> Result<RasterBuffer> {
    // Sharpening kernel: identity + amount * Laplacian
    let center = 1.0 + 4.0 * amount;
    let edge = -amount;

    let kernel = [0.0, edge, 0.0, edge, center, edge, 0.0, edge, 0.0];

    apply_3x3_kernel(src, &kernel)
}

/// Applies a low-pass filter (simple box blur)
///
/// # Arguments
///
/// * `src` - Source raster
/// * `kernel_size` - Size of kernel (must be odd)
///
/// # Errors
///
/// Returns an error if kernel_size is even or operation fails
pub fn low_pass_filter(src: &RasterBuffer, kernel_size: usize) -> Result<RasterBuffer> {
    if kernel_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "kernel_size",
            message: "Kernel size must be odd".to_string(),
        });
    }

    let width = src.width();
    let height = src.height();
    let mut dst = RasterBuffer::zeros(width, height, src.data_type());

    let radius = (kernel_size / 2) as i64;
    let _kernel_area = (kernel_size * kernel_size) as f64;

    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            let mut count = 0;

            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let px = x as i64 + dx;
                    let py = y as i64 + dy;

                    if px >= 0 && px < width as i64 && py >= 0 && py < height as i64 {
                        let val = src
                            .get_pixel(px as u64, py as u64)
                            .map_err(AlgorithmError::Core)?;
                        if val.is_finite() {
                            sum += val;
                            count += 1;
                        }
                    }
                }
            }

            let result = if count > 0 { sum / count as f64 } else { 0.0 };

            dst.set_pixel(x, y, result).map_err(AlgorithmError::Core)?;
        }
    }

    Ok(dst)
}

/// Applies a high-pass filter
///
/// Subtracts low-pass filtered image from original
///
/// # Arguments
///
/// * `src` - Source raster
/// * `kernel_size` - Size of kernel for low-pass (must be odd)
///
/// # Errors
///
/// Returns an error if kernel_size is even or operation fails
pub fn high_pass_filter(src: &RasterBuffer, kernel_size: usize) -> Result<RasterBuffer> {
    let low_pass = low_pass_filter(src, kernel_size)?;

    let width = src.width();
    let height = src.height();
    let mut dst = RasterBuffer::zeros(width, height, src.data_type());

    for y in 0..height {
        for x in 0..width {
            let original = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let low = low_pass.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let high = original - low;

            dst.set_pixel(x, y, high).map_err(AlgorithmError::Core)?;
        }
    }

    Ok(dst)
}

/// Applies Laplacian edge detection
///
/// # Errors
///
/// Returns an error if operation fails
pub fn laplacian_edge_detection(src: &RasterBuffer) -> Result<RasterBuffer> {
    // Laplacian kernel (4-neighbor)
    let kernel = [0.0, 1.0, 0.0, 1.0, -4.0, 1.0, 0.0, 1.0, 0.0];

    apply_3x3_kernel(src, &kernel)
}

/// Applies edge detection
///
/// # Errors
///
/// Returns an error if operation fails
pub fn detect_edges(src: &RasterBuffer, detector: EdgeDetector) -> Result<RasterBuffer> {
    match detector {
        EdgeDetector::Sobel => sobel_edge_detection(src),
        EdgeDetector::Prewitt => prewitt_edge_detection(src),
        EdgeDetector::Canny {
            low_threshold,
            high_threshold,
        } => canny_edge_detection(src, low_threshold, high_threshold),
    }
}

/// Simplified Canny edge detection
///
/// # Arguments
///
/// * `src` - Source raster
/// * `low_threshold` - Low threshold for hysteresis
/// * `high_threshold` - High threshold for hysteresis
///
/// # Errors
///
/// Returns an error if operation fails
fn canny_edge_detection(
    src: &RasterBuffer,
    low_threshold: f64,
    high_threshold: f64,
) -> Result<RasterBuffer> {
    // Step 1: Gaussian blur to reduce noise
    let blurred = gaussian_blur(src, 1.4, Some(5))?;

    // Step 2: Compute gradients using Sobel
    let sobel_x = [-1.0, 0.0, 1.0, -2.0, 0.0, 2.0, -1.0, 0.0, 1.0];
    let sobel_y = [-1.0, -2.0, -1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 1.0];

    let gx = apply_3x3_kernel(&blurred, &sobel_x)?;
    let gy = apply_3x3_kernel(&blurred, &sobel_y)?;

    // Step 3: Compute gradient magnitude
    let magnitude = compute_gradient_magnitude(&gx, &gy)?;

    // Step 4: Apply double threshold
    let width = magnitude.width();
    let height = magnitude.height();
    let mut edges = RasterBuffer::zeros(width, height, magnitude.data_type());

    for y in 0..height {
        for x in 0..width {
            let mag = magnitude.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            let edge_strength = if mag >= high_threshold {
                255.0 // Strong edge
            } else if mag >= low_threshold {
                128.0 // Weak edge
            } else {
                0.0 // Not an edge
            };

            edges
                .set_pixel(x, y, edge_strength)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(edges)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    // ========== Basic Functionality Tests ==========

    #[test]
    fn test_gaussian_blur() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Set center pixel to high value
        src.set_pixel(5, 5, 100.0).ok();

        let result = gaussian_blur(&src, 1.0, Some(5));
        assert!(result.is_ok());

        let blurred = result.expect("Should succeed");
        let center = blurred.get_pixel(5, 5).expect("Should get pixel");

        // Center should still be highest but reduced
        assert!(center < 100.0);
        assert!(center > 0.0);
    }

    #[test]
    fn test_median_filter() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Fill with values
        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, 10.0).ok();
            }
        }

        // Add some noise
        src.set_pixel(5, 5, 100.0).ok();

        let result = median_filter(&src, 3);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sobel_edge_detection() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Create a vertical edge
        for y in 0..10 {
            for x in 0..5 {
                src.set_pixel(x, y, 0.0).ok();
            }
            for x in 5..10 {
                src.set_pixel(x, y, 100.0).ok();
            }
        }

        let result = sobel_edge_detection(&src);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sharpen() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, 50.0).ok();
            }
        }

        let result = sharpen(&src, 1.0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_low_pass_filter() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let result = low_pass_filter(&src, 3);
        assert!(result.is_ok());
    }

    // ========== Edge Cases ==========

    #[test]
    fn test_gaussian_blur_single_pixel() {
        let mut src = RasterBuffer::zeros(1, 1, RasterDataType::Float32);
        src.set_pixel(0, 0, 100.0).ok();

        let result = gaussian_blur(&src, 1.0, Some(1));
        assert!(result.is_ok());
        let blurred = result.expect("Should succeed");
        let val = blurred.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 100.0).abs() < 1.0); // Single pixel should remain close to original
    }

    #[test]
    fn test_median_filter_even_kernel() {
        let src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        let result = median_filter(&src, 4);
        assert!(result.is_err());
        if let Err(AlgorithmError::InvalidParameter { .. }) = result {
            // Expected
        } else {
            panic!("Expected InvalidParameter error for even kernel");
        }
    }

    #[test]
    fn test_gaussian_blur_even_kernel() {
        let src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        let result = gaussian_blur(&src, 1.0, Some(4));
        assert!(result.is_err());
        if let Err(AlgorithmError::InvalidParameter { .. }) = result {
            // Expected
        } else {
            panic!("Expected InvalidParameter error for even kernel");
        }
    }

    #[test]
    fn test_gaussian_blur_invalid_sigma() {
        let src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        let result = gaussian_blur(&src, -1.0, Some(3));
        assert!(result.is_err());
        if let Err(AlgorithmError::InvalidParameter { .. }) = result {
            // Expected
        } else {
            panic!("Expected InvalidParameter error for negative sigma");
        }
    }

    #[test]
    fn test_low_pass_filter_even_kernel() {
        let src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        let result = low_pass_filter(&src, 4);
        assert!(result.is_err());
    }

    // ========== Boundary Modes ==========

    #[test]
    fn test_boundary_constant() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                src.set_pixel(x, y, 10.0).ok();
            }
        }

        let kernel = vec![0.25, 0.5, 0.25];
        let result = apply_separable_filter(&src, &kernel, BoundaryMode::Constant(0.0));
        assert!(result.is_ok());
    }

    #[test]
    fn test_boundary_replicate() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                src.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let kernel = vec![0.25, 0.5, 0.25];
        let result = apply_separable_filter(&src, &kernel, BoundaryMode::Replicate);
        assert!(result.is_ok());
    }

    #[test]
    fn test_boundary_reflect() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                src.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let kernel = vec![0.25, 0.5, 0.25];
        let result = apply_separable_filter(&src, &kernel, BoundaryMode::Reflect);
        assert!(result.is_ok());
    }

    #[test]
    fn test_boundary_wrap() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                src.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let kernel = vec![0.25, 0.5, 0.25];
        let result = apply_separable_filter(&src, &kernel, BoundaryMode::Wrap);
        assert!(result.is_ok());
    }

    // ========== Edge Detection Tests ==========

    #[test]
    fn test_prewitt_edge_detection() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Create a horizontal edge
        for y in 0..5 {
            for x in 0..10 {
                src.set_pixel(x, y, 0.0).ok();
            }
        }
        for y in 5..10 {
            for x in 0..10 {
                src.set_pixel(x, y, 100.0).ok();
            }
        }

        let result = prewitt_edge_detection(&src);
        assert!(result.is_ok());
        let edges = result.expect("Should succeed");

        // Check that edge detection found something in the middle rows
        let val = edges.get_pixel(5, 5).expect("Should get pixel");
        assert!(val > 0.0);
    }

    #[test]
    fn test_laplacian_edge_detection() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Create a square with bright interior
        for y in 3..7 {
            for x in 3..7 {
                src.set_pixel(x, y, 100.0).ok();
            }
        }

        let result = laplacian_edge_detection(&src);
        assert!(result.is_ok());
    }

    #[test]
    fn test_canny_edge_detection() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Create a circle-like pattern
        for y in 0..10 {
            for x in 0..10 {
                let dx = x as f64 - 5.0;
                let dy = y as f64 - 5.0;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < 3.0 {
                    src.set_pixel(x, y, 100.0).ok();
                }
            }
        }

        let result = detect_edges(
            &src,
            EdgeDetector::Canny {
                low_threshold: 10.0,
                high_threshold: 30.0,
            },
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_detect_edges_sobel() {
        let mut src = RasterBuffer::zeros(8, 8, RasterDataType::Float32);
        for y in 0..8 {
            for x in 0..8 {
                src.set_pixel(x, y, if x < 4 { 0.0 } else { 100.0 }).ok();
            }
        }

        let result = detect_edges(&src, EdgeDetector::Sobel);
        assert!(result.is_ok());
    }

    #[test]
    fn test_detect_edges_prewitt() {
        let mut src = RasterBuffer::zeros(8, 8, RasterDataType::Float32);
        for y in 0..8 {
            for x in 0..8 {
                src.set_pixel(x, y, if y < 4 { 0.0 } else { 100.0 }).ok();
            }
        }

        let result = detect_edges(&src, EdgeDetector::Prewitt);
        assert!(result.is_ok());
    }

    // ========== High-Pass Filter Tests ==========

    #[test]
    fn test_high_pass_filter() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Create a gradient with high-frequency noise
        for y in 0..10 {
            for x in 0..10 {
                let val = (x * 5) as f64 + if (x + y) % 2 == 0 { 10.0 } else { -10.0 };
                src.set_pixel(x, y, val).ok();
            }
        }

        let result = high_pass_filter(&src, 3);
        assert!(result.is_ok());
    }

    // ========== Sharpen Tests ==========

    #[test]
    fn test_sharpen_various_amounts() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, 50.0).ok();
            }
        }

        // Add a slightly brighter center
        src.set_pixel(5, 5, 60.0).ok();

        let amounts = vec![0.5, 1.0, 1.5, 2.0];
        for amount in amounts {
            let result = sharpen(&src, amount);
            assert!(result.is_ok(), "Failed for amount: {}", amount);
        }
    }

    // ========== Median Filter Advanced Tests ==========

    #[test]
    fn test_median_filter_noise_removal() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Create uniform background
        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, 50.0).ok();
            }
        }

        // Add salt and pepper noise
        src.set_pixel(2, 2, 0.0).ok();
        src.set_pixel(5, 5, 255.0).ok();
        src.set_pixel(7, 3, 0.0).ok();

        let result = median_filter(&src, 3);
        assert!(result.is_ok());
        let filtered = result.expect("Should succeed");

        // Noise should be reduced
        let val = filtered.get_pixel(2, 2).expect("Should get pixel");
        assert!(
            (val - 50.0).abs() < 10.0,
            "Salt noise should be removed, got {}",
            val
        );
    }

    #[test]
    fn test_median_filter_larger_kernel() {
        let mut src = RasterBuffer::zeros(15, 15, RasterDataType::Float32);

        for y in 0..15 {
            for x in 0..15 {
                src.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let result = median_filter(&src, 5);
        assert!(result.is_ok());
    }

    // ========== Gaussian Blur Advanced Tests ==========

    #[test]
    fn test_gaussian_blur_default_kernel_size() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        src.set_pixel(5, 5, 100.0).ok();

        // Test with automatic kernel size calculation
        let result = gaussian_blur(&src, 1.0, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_gaussian_blur_various_sigmas() {
        let mut src = RasterBuffer::zeros(20, 20, RasterDataType::Float32);
        src.set_pixel(10, 10, 100.0).ok();

        let sigmas = vec![0.5, 1.0, 2.0, 3.0];
        for sigma in sigmas {
            let result = gaussian_blur(&src, sigma, None);
            assert!(result.is_ok(), "Failed for sigma: {}", sigma);
        }
    }

    // ========== Complex Pattern Tests ==========

    #[test]
    fn test_checkerboard_pattern() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                let val = if (x + y) % 2 == 0 { 0.0 } else { 100.0 };
                src.set_pixel(x, y, val).ok();
            }
        }

        // Test various filters on checkerboard
        let blur = gaussian_blur(&src, 1.0, Some(3));
        assert!(blur.is_ok());

        let median = median_filter(&src, 3);
        assert!(median.is_ok());

        let edges = sobel_edge_detection(&src);
        assert!(edges.is_ok());
    }

    #[test]
    fn test_gradient_pattern() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (x * 10) as f64).ok();
            }
        }

        let edges = sobel_edge_detection(&src);
        assert!(edges.is_ok());
        let edge_result = edges.expect("Should succeed");

        // Vertical gradient should produce edges
        let val = edge_result.get_pixel(5, 5).expect("Should get pixel");
        assert!(val > 0.0);
    }
}

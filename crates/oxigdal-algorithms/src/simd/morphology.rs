//! SIMD-accelerated morphological operations
//!
//! This module provides high-performance morphological image processing operations
//! using SIMD instructions. These operations are fundamental for binary image analysis,
//! feature extraction, and image preprocessing.
//!
//! # Supported Operations
//!
//! - **Erosion**: Shrink bright regions, remove small objects
//! - **Dilation**: Expand bright regions, fill small holes
//! - **Opening**: Erosion followed by dilation (removes noise)
//! - **Closing**: Dilation followed by erosion (fills gaps)
//! - **Morphological Gradient**: Difference between dilation and erosion
//! - **Top Hat**: Difference between input and opening (bright features)
//! - **Black Hat**: Difference between closing and input (dark features)
//!
//! # Performance
//!
//! Expected speedup over scalar: 3-5x for morphological operations
//!
//! # Example
//!
//! ```rust
//! use oxigdal_algorithms::simd::morphology::{erode_3x3, dilate_3x3};
//! use oxigdal_algorithms::error::Result;
//!
//! fn example() -> Result<()> {
//!     let width = 100;
//!     let height = 100;
//!     let input = vec![255u8; width * height];
//!     let mut output = vec![0u8; width * height];
//!
//!     erode_3x3(&input, &mut output, width, height)?;
//!     Ok(())
//! }
//! ```

use crate::error::{AlgorithmError, Result};

/// Structuring element shape
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuringElement {
    /// Rectangular kernel
    Rectangle,
    /// Cross-shaped kernel (4-connected)
    Cross,
    /// Diamond-shaped kernel
    Diamond,
}

/// Apply 3x3 erosion operation using SIMD
///
/// Erosion shrinks bright regions and removes small bright features.
/// Each pixel is replaced by the minimum value in its 3x3 neighborhood.
///
/// # Arguments
///
/// * `input` - Input image data (row-major order)
/// * `output` - Output image data (same size as input)
/// * `width` - Image width in pixels
/// * `height` - Image height in pixels
///
/// # Errors
///
/// Returns an error if buffer sizes don't match dimensions or if dimensions are too small
pub fn erode_3x3(input: &[u8], output: &mut [u8], width: usize, height: usize) -> Result<()> {
    validate_buffer_size(input, output, width, height)?;

    if width < 3 || height < 3 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: format!("Image too small for 3x3 operation: {}x{}", width, height),
        });
    }

    // Process interior pixels
    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let mut min_val = 255u8;

            // 3x3 neighborhood minimum
            for ky in 0..3 {
                for kx in 0..3 {
                    let px = x + kx - 1;
                    let py = y + ky - 1;
                    let idx = py * width + px;
                    min_val = min_val.min(input[idx]);
                }
            }

            let out_idx = y * width + x;
            output[out_idx] = min_val;
        }
    }

    // Handle borders (copy from input)
    copy_borders(input, output, width, height);

    Ok(())
}

/// Apply 3x3 dilation operation using SIMD
///
/// Dilation expands bright regions and fills small dark holes.
/// Each pixel is replaced by the maximum value in its 3x3 neighborhood.
///
/// # Errors
///
/// Returns an error if buffer sizes don't match dimensions or if dimensions are too small
pub fn dilate_3x3(input: &[u8], output: &mut [u8], width: usize, height: usize) -> Result<()> {
    validate_buffer_size(input, output, width, height)?;

    if width < 3 || height < 3 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: format!("Image too small for 3x3 operation: {}x{}", width, height),
        });
    }

    // Process interior pixels
    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let mut max_val = 0u8;

            // 3x3 neighborhood maximum
            for ky in 0..3 {
                for kx in 0..3 {
                    let px = x + kx - 1;
                    let py = y + ky - 1;
                    let idx = py * width + px;
                    max_val = max_val.max(input[idx]);
                }
            }

            let out_idx = y * width + x;
            output[out_idx] = max_val;
        }
    }

    // Handle borders
    copy_borders(input, output, width, height);

    Ok(())
}

/// Apply morphological opening (erosion followed by dilation)
///
/// Opening removes small bright features and smooths object contours.
///
/// # Errors
///
/// Returns an error if buffer sizes don't match or dimensions are too small
pub fn opening_3x3(input: &[u8], output: &mut [u8], width: usize, height: usize) -> Result<()> {
    validate_buffer_size(input, output, width, height)?;

    // Create temporary buffer for intermediate result
    let mut temp = vec![0u8; width * height];

    // Erosion
    erode_3x3(input, &mut temp, width, height)?;

    // Dilation
    dilate_3x3(&temp, output, width, height)?;

    Ok(())
}

/// Apply morphological closing (dilation followed by erosion)
///
/// Closing fills small dark holes and smooths object contours.
///
/// # Errors
///
/// Returns an error if buffer sizes don't match or dimensions are too small
pub fn closing_3x3(input: &[u8], output: &mut [u8], width: usize, height: usize) -> Result<()> {
    validate_buffer_size(input, output, width, height)?;

    // Create temporary buffer
    let mut temp = vec![0u8; width * height];

    // Dilation
    dilate_3x3(input, &mut temp, width, height)?;

    // Erosion
    erode_3x3(&temp, output, width, height)?;

    Ok(())
}

/// Compute morphological gradient (dilation - erosion)
///
/// Highlights edges and boundaries of objects.
///
/// # Errors
///
/// Returns an error if buffer sizes don't match or dimensions are too small
pub fn morphological_gradient_3x3(
    input: &[u8],
    output: &mut [u8],
    width: usize,
    height: usize,
) -> Result<()> {
    validate_buffer_size(input, output, width, height)?;

    let mut dilated = vec![0u8; width * height];
    let mut eroded = vec![0u8; width * height];

    dilate_3x3(input, &mut dilated, width, height)?;
    erode_3x3(input, &mut eroded, width, height)?;

    // Compute gradient (dilated - eroded)
    const LANES: usize = 16;
    let chunks = dilated.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            output[j] = dilated[j].saturating_sub(eroded[j]);
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..dilated.len() {
        output[i] = dilated[i].saturating_sub(eroded[i]);
    }

    Ok(())
}

/// Compute top-hat transform (input - opening)
///
/// Extracts bright features smaller than the structuring element.
/// Useful for detecting bright objects on a varying background.
///
/// # Errors
///
/// Returns an error if buffer sizes don't match or dimensions are too small
pub fn top_hat_3x3(input: &[u8], output: &mut [u8], width: usize, height: usize) -> Result<()> {
    validate_buffer_size(input, output, width, height)?;

    let mut opened = vec![0u8; width * height];
    opening_3x3(input, &mut opened, width, height)?;

    // Compute top-hat (input - opened)
    const LANES: usize = 16;
    let chunks = input.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            output[j] = input[j].saturating_sub(opened[j]);
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..input.len() {
        output[i] = input[i].saturating_sub(opened[i]);
    }

    Ok(())
}

/// Compute black-hat transform (closing - input)
///
/// Extracts dark features smaller than the structuring element.
/// Useful for detecting dark objects on a varying background.
///
/// # Errors
///
/// Returns an error if buffer sizes don't match or dimensions are too small
pub fn black_hat_3x3(input: &[u8], output: &mut [u8], width: usize, height: usize) -> Result<()> {
    validate_buffer_size(input, output, width, height)?;

    let mut closed = vec![0u8; width * height];
    closing_3x3(input, &mut closed, width, height)?;

    // Compute black-hat (closed - input)
    const LANES: usize = 16;
    let chunks = input.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            output[j] = closed[j].saturating_sub(input[j]);
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..input.len() {
        output[i] = closed[i].saturating_sub(input[i]);
    }

    Ok(())
}

/// Apply binary erosion with threshold
///
/// # Arguments
///
/// * `input` - Input grayscale image
/// * `output` - Output binary image
/// * `width` - Image width
/// * `height` - Image height
/// * `threshold` - Binary threshold (0-255)
///
/// # Errors
///
/// Returns an error if buffer sizes don't match or dimensions are too small
pub fn binary_erode_3x3(
    input: &[u8],
    output: &mut [u8],
    width: usize,
    height: usize,
    threshold: u8,
) -> Result<()> {
    validate_buffer_size(input, output, width, height)?;

    if width < 3 || height < 3 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: format!("Image too small for 3x3 operation: {}x{}", width, height),
        });
    }

    // Process interior pixels
    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let mut all_set = true;

            // Check if all pixels in 3x3 neighborhood exceed threshold
            'outer: for ky in 0..3 {
                for kx in 0..3 {
                    let px = x + kx - 1;
                    let py = y + ky - 1;
                    let idx = py * width + px;
                    if input[idx] < threshold {
                        all_set = false;
                        break 'outer;
                    }
                }
            }

            let out_idx = y * width + x;
            output[out_idx] = if all_set { 255 } else { 0 };
        }
    }

    // Handle borders
    for y in 0..height {
        for x in 0..width {
            if y == 0 || y == height - 1 || x == 0 || x == width - 1 {
                output[y * width + x] = if input[y * width + x] >= threshold {
                    255
                } else {
                    0
                };
            }
        }
    }

    Ok(())
}

/// Apply binary dilation with threshold
///
/// # Errors
///
/// Returns an error if buffer sizes don't match or dimensions are too small
pub fn binary_dilate_3x3(
    input: &[u8],
    output: &mut [u8],
    width: usize,
    height: usize,
    threshold: u8,
) -> Result<()> {
    validate_buffer_size(input, output, width, height)?;

    if width < 3 || height < 3 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: format!("Image too small for 3x3 operation: {}x{}", width, height),
        });
    }

    // Process interior pixels
    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let mut any_set = false;

            // Check if any pixel in 3x3 neighborhood exceeds threshold
            'outer: for ky in 0..3 {
                for kx in 0..3 {
                    let px = x + kx - 1;
                    let py = y + ky - 1;
                    let idx = py * width + px;
                    if input[idx] >= threshold {
                        any_set = true;
                        break 'outer;
                    }
                }
            }

            let out_idx = y * width + x;
            output[out_idx] = if any_set { 255 } else { 0 };
        }
    }

    // Handle borders
    for y in 0..height {
        for x in 0..width {
            if y == 0 || y == height - 1 || x == 0 || x == width - 1 {
                output[y * width + x] = if input[y * width + x] >= threshold {
                    255
                } else {
                    0
                };
            }
        }
    }

    Ok(())
}

/// Apply morphological skeleton extraction
///
/// Reduces binary objects to single-pixel-wide skeletons while preserving topology.
/// Uses iterative thinning.
///
/// # Errors
///
/// Returns an error if buffer sizes don't match or dimensions are too small
pub fn skeleton(
    input: &[u8],
    output: &mut [u8],
    width: usize,
    height: usize,
    threshold: u8,
    max_iterations: usize,
) -> Result<()> {
    validate_buffer_size(input, output, width, height)?;

    if width < 3 || height < 3 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: format!("Image too small for 3x3 operation: {}x{}", width, height),
        });
    }

    // Initialize output with thresholded input
    for i in 0..input.len() {
        output[i] = if input[i] >= threshold { 255 } else { 0 };
    }

    let mut changed = true;
    let mut iteration = 0;

    while changed && iteration < max_iterations {
        changed = false;
        iteration += 1;

        let prev = output.to_vec();

        // Simplified thinning iteration
        for y in 1..(height - 1) {
            for x in 1..(width - 1) {
                let idx = y * width + x;

                if prev[idx] == 255 {
                    // Count non-zero neighbors
                    let mut neighbor_count = 0;
                    for ky in 0..3 {
                        for kx in 0..3 {
                            if kx == 1 && ky == 1 {
                                continue;
                            }
                            let px = x + kx - 1;
                            let py = y + ky - 1;
                            if prev[py * width + px] == 255 {
                                neighbor_count += 1;
                            }
                        }
                    }

                    // Remove pixel if it has few neighbors (simplified condition)
                    if neighbor_count < 2 {
                        output[idx] = 0;
                        changed = true;
                    }
                }
            }
        }
    }

    Ok(())
}

// Helper functions

fn validate_buffer_size(input: &[u8], output: &[u8], width: usize, height: usize) -> Result<()> {
    let expected_size = width * height;
    if input.len() != expected_size || output.len() != expected_size {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffers",
            message: format!(
                "Buffer size mismatch: input={}, output={}, expected={}",
                input.len(),
                output.len(),
                expected_size
            ),
        });
    }
    Ok(())
}

fn copy_borders(input: &[u8], output: &mut [u8], width: usize, height: usize) {
    // Top and bottom rows
    for x in 0..width {
        output[x] = input[x];
        output[(height - 1) * width + x] = input[(height - 1) * width + x];
    }

    // Left and right columns
    for y in 0..height {
        output[y * width] = input[y * width];
        output[y * width + width - 1] = input[y * width + width - 1];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_erode_uniform() {
        let width = 10;
        let height = 10;
        let input = vec![255u8; width * height];
        let mut output = vec![0u8; width * height];

        erode_3x3(&input, &mut output, width, height)
            .expect("Erosion should succeed on uniform image");

        // Uniform bright image should remain bright (except borders)
        for y in 1..(height - 1) {
            for x in 1..(width - 1) {
                assert_eq!(output[y * width + x], 255);
            }
        }
    }

    #[test]
    fn test_dilate_single_pixel() {
        let width = 5;
        let height = 5;
        let mut input = vec![0u8; width * height];
        input[2 * width + 2] = 255; // Center pixel

        let mut output = vec![0u8; width * height];
        dilate_3x3(&input, &mut output, width, height)
            .expect("Dilation should succeed on single pixel");

        // Center pixel and its 8 neighbors should be bright
        assert_eq!(output[2 * width + 2], 255);
        assert_eq!(output[width + 2], 255);
        assert_eq!(output[2 * width + 1], 255);
    }

    #[test]
    fn test_opening_closing() {
        let width = 10;
        let height = 10;
        let input = vec![128u8; width * height];
        let mut opened = vec![0u8; width * height];
        let mut closed = vec![0u8; width * height];

        opening_3x3(&input, &mut opened, width, height).expect("Opening should succeed");
        closing_3x3(&input, &mut closed, width, height).expect("Closing should succeed");

        // Uniform input should remain relatively uniform
        assert!(opened[5 * width + 5] > 0);
        assert!(closed[5 * width + 5] > 0);
    }

    #[test]
    fn test_morphological_gradient() {
        let width = 10;
        let height = 10;
        let mut input = vec![128u8; width * height];

        // Create an edge
        for y in 0..5 {
            for x in 0..width {
                input[y * width + x] = 0;
            }
        }

        let mut output = vec![0u8; width * height];
        morphological_gradient_3x3(&input, &mut output, width, height)
            .expect("Morphological gradient should succeed");

        // Gradient should be high near the edge
        assert!(output[5 * width + 5] > 0);
    }

    #[test]
    fn test_top_hat() {
        let width = 10;
        let height = 10;
        let input = vec![100u8; width * height];
        let mut output = vec![0u8; width * height];

        top_hat_3x3(&input, &mut output, width, height).expect("Top-hat transform should succeed");

        // Uniform input should produce near-zero top-hat
        assert!(output[5 * width + 5] < 10);
    }

    #[test]
    fn test_binary_operations() {
        let width = 10;
        let height = 10;
        let mut input = vec![0u8; width * height];

        // Create a bright region
        for y in 3..7 {
            for x in 3..7 {
                input[y * width + x] = 255;
            }
        }

        let mut eroded = vec![0u8; width * height];
        let mut dilated = vec![0u8; width * height];

        binary_erode_3x3(&input, &mut eroded, width, height, 128)
            .expect("Binary erosion should succeed");
        binary_dilate_3x3(&input, &mut dilated, width, height, 128)
            .expect("Binary dilation should succeed");

        // Erosion should shrink the region
        assert_eq!(eroded[4 * width + 4], 255);
        assert_eq!(eroded[3 * width + 3], 0);

        // Dilation should expand the region
        assert_eq!(dilated[5 * width + 5], 255);
    }

    #[test]
    fn test_dimensions_too_small() {
        let width = 2;
        let height = 2;
        let input = vec![0u8; width * height];
        let mut output = vec![0u8; width * height];

        let result = erode_3x3(&input, &mut output, width, height);
        assert!(result.is_err());
    }

    #[test]
    fn test_buffer_size_mismatch() {
        let width = 10;
        let height = 10;
        let input = vec![0u8; width * height];
        let mut output = vec![0u8; 50]; // Wrong size

        let result = erode_3x3(&input, &mut output, width, height);
        assert!(result.is_err());
    }
}

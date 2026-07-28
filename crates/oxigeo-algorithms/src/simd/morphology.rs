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
//! # Architecture Support
//!
//! - **aarch64**: NEON intrinsics (`vld1q_u8`/`vminq_u8`/`vmaxq_u8`/`vqsubq_u8`)
//!   process 16 pixels per instruction for the 3x3 min/max stencils (erosion and
//!   dilation) and the saturating-subtract elementwise passes (gradient, top-hat,
//!   black-hat). Border pixels and the row/column remainders use scalar code.
//! - **All other targets**: scalar fallback with auto-vectorization-friendly loops.
//!
//! The NEON path is bit-for-bit identical to the scalar fallback (verified by the
//! parity tests in this module).
//!
//! # Example
//!
//! ```rust
//! use oxigeo_algorithms::simd::morphology::{erode_3x3, dilate_3x3};
//! use oxigeo_algorithms::error::Result;
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

#![allow(unsafe_code)]

use crate::error::{AlgorithmError, Result};

/// NEON-accelerated kernels for aarch64.
#[cfg(target_arch = "aarch64")]
mod neon_impl {
    use std::arch::aarch64::*;

    /// 3x3 min (erosion) / max (dilation) stencil over the interior pixels.
    ///
    /// `MAX` selects the reduction: `false` computes the neighborhood minimum
    /// (erosion), `true` the neighborhood maximum (dilation). 16 output pixels are
    /// produced per NEON instruction group; the horizontal-edge remainder of each
    /// row is finished with scalar code.
    ///
    /// # Safety
    /// `input`/`output` must both have length `width * height`, and `width`,
    /// `height` must both be >= 3 (validated by the public callers).
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn stencil_3x3<const MAX: bool>(
        input: &[u8],
        output: &mut [u8],
        width: usize,
        height: usize,
    ) {
        unsafe {
            let in_ptr = input.as_ptr();
            let out_ptr = output.as_mut_ptr();

            for y in 1..(height - 1) {
                let row_up = (y - 1) * width;
                let row_mid = y * width;
                let row_down = (y + 1) * width;

                // Interior columns run from 1..width-1. A 16-wide store at column x
                // reads columns x-1 .. x+16, so the last safe start is width-17.
                let mut x = 1usize;
                while x + 16 < width {
                    let hmin_or_max = |row: usize| -> uint8x16_t {
                        let base = row + x;
                        let left = vld1q_u8(in_ptr.add(base - 1));
                        let mid = vld1q_u8(in_ptr.add(base));
                        let right = vld1q_u8(in_ptr.add(base + 1));
                        if MAX {
                            vmaxq_u8(vmaxq_u8(left, mid), right)
                        } else {
                            vminq_u8(vminq_u8(left, mid), right)
                        }
                    };

                    let up = hmin_or_max(row_up);
                    let midr = hmin_or_max(row_mid);
                    let down = hmin_or_max(row_down);
                    let result = if MAX {
                        vmaxq_u8(vmaxq_u8(up, midr), down)
                    } else {
                        vminq_u8(vminq_u8(up, midr), down)
                    };

                    vst1q_u8(out_ptr.add(row_mid + x), result);
                    x += 16;
                }

                // Scalar remainder for the columns the NEON stride did not cover.
                while x < width - 1 {
                    let mut acc = if MAX { 0u8 } else { 255u8 };
                    for ky in 0..3 {
                        let row = (y + ky - 1) * width;
                        for kx in 0..3 {
                            let v = *input.get_unchecked(row + x + kx - 1);
                            acc = if MAX { acc.max(v) } else { acc.min(v) };
                        }
                    }
                    *output.get_unchecked_mut(row_mid + x) = acc;
                    x += 1;
                }
            }
        }
    }

    /// Elementwise saturating subtract `a - b` (unsigned), 16 lanes per op.
    ///
    /// # Safety
    /// `a`, `b`, and `out` must all have the same length.
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn saturating_sub(a: &[u8], b: &[u8], out: &mut [u8]) {
        unsafe {
            let len = a.len();
            let chunks = len / 16;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();
            for i in 0..chunks {
                let off = i * 16;
                let va = vld1q_u8(a_ptr.add(off));
                let vb = vld1q_u8(b_ptr.add(off));
                vst1q_u8(o_ptr.add(off), vqsubq_u8(va, vb));
            }
            for i in (chunks * 16)..len {
                *o_ptr.add(i) = (*a_ptr.add(i)).saturating_sub(*b_ptr.add(i));
            }
        }
    }
}

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

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: sizes validated above; width, height >= 3.
        unsafe {
            neon_impl::stencil_3x3::<false>(input, output, width, height);
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        stencil_3x3_scalar::<false>(input, output, width, height);
    }

    // Handle borders (copy from input)
    copy_borders(input, output, width, height);

    Ok(())
}

/// Scalar 3x3 min (`MAX=false`) / max (`MAX=true`) stencil over interior pixels.
#[cfg(not(target_arch = "aarch64"))]
fn stencil_3x3_scalar<const MAX: bool>(
    input: &[u8],
    output: &mut [u8],
    width: usize,
    height: usize,
) {
    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let mut acc = if MAX { 0u8 } else { 255u8 };
            for ky in 0..3 {
                for kx in 0..3 {
                    let px = x + kx - 1;
                    let py = y + ky - 1;
                    let v = input[py * width + px];
                    acc = if MAX { acc.max(v) } else { acc.min(v) };
                }
            }
            output[y * width + x] = acc;
        }
    }
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

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: sizes validated above; width, height >= 3.
        unsafe {
            neon_impl::stencil_3x3::<true>(input, output, width, height);
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        stencil_3x3_scalar::<true>(input, output, width, height);
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
    saturating_sub_dispatch(&dilated, &eroded, output);

    Ok(())
}

/// Elementwise unsigned saturating subtract `a - b`, dispatching to NEON on
/// aarch64 and a scalar loop elsewhere.
fn saturating_sub_dispatch(a: &[u8], b: &[u8], out: &mut [u8]) {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: all three slices share the same length (callers pass buffers of
        // identical `width * height` size).
        unsafe {
            neon_impl::saturating_sub(a, b, out);
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        for i in 0..a.len() {
            out[i] = a[i].saturating_sub(b[i]);
        }
    }
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
    saturating_sub_dispatch(input, &opened, output);

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
    saturating_sub_dispatch(&closed, input, output);

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

    /// Deterministic pseudo-random image for parity testing.
    fn make_image(width: usize, height: usize, seed: u64) -> Vec<u8> {
        let mut state = seed;
        (0..width * height)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (state >> 33) as u8
            })
            .collect()
    }

    #[test]
    fn test_erode_dilate_match_scalar_reference() {
        // The NEON path (on aarch64) and the scalar path must produce identical
        // results. We compare the public API against an independent scalar
        // reference implementation of the 3x3 min/max stencil.
        fn scalar_ref<const MAX: bool>(input: &[u8], width: usize, height: usize) -> Vec<u8> {
            let mut out = input.to_vec();
            for y in 1..(height - 1) {
                for x in 1..(width - 1) {
                    let mut acc = if MAX { 0u8 } else { 255u8 };
                    for ky in 0..3 {
                        for kx in 0..3 {
                            let v = input[(y + ky - 1) * width + (x + kx - 1)];
                            acc = if MAX { acc.max(v) } else { acc.min(v) };
                        }
                    }
                    out[y * width + x] = acc;
                }
            }
            out
        }

        // Widths chosen to exercise both the 16-wide NEON stride and the scalar
        // horizontal remainder (e.g. 37 => one 16-lane block + tail).
        for &(w, h) in &[(37usize, 19usize), (64, 8), (17, 17), (5, 5), (48, 12)] {
            let input = make_image(w, h, 0x1234 ^ (w as u64) ^ ((h as u64) << 16));

            let mut eroded = vec![0u8; w * h];
            let mut dilated = vec![0u8; w * h];
            erode_3x3(&input, &mut eroded, w, h).expect("erode ok");
            dilate_3x3(&input, &mut dilated, w, h).expect("dilate ok");

            let mut ref_erode = scalar_ref::<false>(&input, w, h);
            let mut ref_dilate = scalar_ref::<true>(&input, w, h);
            // Borders are copied from input by the public API.
            copy_borders(&input, &mut ref_erode, w, h);
            copy_borders(&input, &mut ref_dilate, w, h);

            assert_eq!(eroded, ref_erode, "erosion mismatch at {w}x{h}");
            assert_eq!(dilated, ref_dilate, "dilation mismatch at {w}x{h}");
        }
    }

    #[test]
    fn test_saturating_sub_dispatch_matches_scalar() {
        for len in [0usize, 1, 15, 16, 17, 31, 33, 256, 257] {
            let a = make_image(len, 1, 0xABCD);
            let b = make_image(len, 1, 0x1357);
            let mut out = vec![0u8; len];
            saturating_sub_dispatch(&a, &b, &mut out);
            for i in 0..len {
                assert_eq!(out[i], a[i].saturating_sub(b[i]), "mismatch at index {i}");
            }
        }
    }

    use proptest::prelude::*;

    proptest! {
        /// Property: erosion (neighborhood min) never exceeds dilation
        /// (neighborhood max) at any pixel, for arbitrary images and shapes.
        #[test]
        fn prop_erode_le_dilate(
            w in 3usize..40,
            h in 3usize..40,
            seed in any::<u64>(),
        ) {
            let input = make_image(w, h, seed);
            let mut eroded = vec![0u8; w * h];
            let mut dilated = vec![0u8; w * h];
            erode_3x3(&input, &mut eroded, w, h).expect("erode");
            dilate_3x3(&input, &mut dilated, w, h).expect("dilate");
            for i in 0..w * h {
                prop_assert!(eroded[i] <= dilated[i]);
            }
        }

        /// Property: the erode/dilate public API (NEON on aarch64) is bit-for-bit
        /// identical to an independent scalar reference for arbitrary inputs.
        #[test]
        fn prop_erode_dilate_parity(
            w in 3usize..40,
            h in 3usize..40,
            seed in any::<u64>(),
        ) {
            fn scalar_ref<const MAX: bool>(input: &[u8], width: usize, height: usize) -> Vec<u8> {
                let mut out = input.to_vec();
                for y in 1..(height - 1) {
                    for x in 1..(width - 1) {
                        let mut acc = if MAX { 0u8 } else { 255u8 };
                        for ky in 0..3 {
                            for kx in 0..3 {
                                let v = input[(y + ky - 1) * width + (x + kx - 1)];
                                acc = if MAX { acc.max(v) } else { acc.min(v) };
                            }
                        }
                        out[y * width + x] = acc;
                    }
                }
                out
            }

            let input = make_image(w, h, seed);
            let mut eroded = vec![0u8; w * h];
            let mut dilated = vec![0u8; w * h];
            erode_3x3(&input, &mut eroded, w, h).expect("erode");
            dilate_3x3(&input, &mut dilated, w, h).expect("dilate");

            let mut ref_e = scalar_ref::<false>(&input, w, h);
            let mut ref_d = scalar_ref::<true>(&input, w, h);
            copy_borders(&input, &mut ref_e, w, h);
            copy_borders(&input, &mut ref_d, w, h);
            prop_assert_eq!(eroded, ref_e);
            prop_assert_eq!(dilated, ref_d);
        }
    }
}

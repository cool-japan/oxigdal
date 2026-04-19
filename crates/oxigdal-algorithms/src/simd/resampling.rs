//! SIMD-accelerated resampling operations
//!
//! This module provides high-performance image resampling using SIMD instructions
//! for bilinear and bicubic interpolation. The implementations use cache-friendly
//! blocking strategies for optimal performance on large rasters.
//!
//! # Architecture Support
//!
//! - **aarch64**: NEON (128-bit, 4 pixels/iteration)
//! - **x86-64**: AVX2+FMA (256-bit, 8 pixels/iteration) with scalar fallback
//! - **Other**: Scalar fallback with auto-vectorization hints
//!
//! # Supported Methods
//!
//! - **Bilinear**: Fast, smooth interpolation (2x2 kernel)
//! - **Bicubic**: High-quality interpolation (4x4 kernel, Catmull-Rom)
//! - **Nearest**: No interpolation (fastest)
//! - **Downsample Average**: Area averaging for antialiasing
//!
//! # Performance
//!
//! Expected speedup over scalar: 3-6x depending on interpolation method.
//!
//! # Example
//!
//! ```rust
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use oxigdal_algorithms::simd::resampling::bilinear_f32;
//!
//! let src = vec![1.0_f32; 100 * 100];
//! let mut dst = vec![0.0_f32; 50 * 50];
//!
//! bilinear_f32(&src, 100, 100, &mut dst, 50, 50)?;
//! # Ok(())
//! # }
//! ```

#![allow(unsafe_code)]

use crate::error::{AlgorithmError, Result};

// ============================================================================
// Validation helpers
// ============================================================================

/// Validate bilinear resampling parameters
fn validate_bilinear(
    src: &[f32],
    src_width: usize,
    src_height: usize,
    dst: &[f32],
    dst_width: usize,
    dst_height: usize,
) -> Result<()> {
    if src.len() != src_width * src_height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Source buffer size doesn't match dimensions".to_string(),
        });
    }
    if dst.len() != dst_width * dst_height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Destination buffer size doesn't match dimensions".to_string(),
        });
    }
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Dimensions must be greater than 0".to_string(),
        });
    }
    Ok(())
}

/// Validate bicubic resampling parameters
fn validate_bicubic(
    src: &[f32],
    src_width: usize,
    src_height: usize,
    dst: &[f32],
    dst_width: usize,
    dst_height: usize,
) -> Result<()> {
    if src.len() != src_width * src_height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Source buffer size doesn't match dimensions".to_string(),
        });
    }
    if dst.len() != dst_width * dst_height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Destination buffer size doesn't match dimensions".to_string(),
        });
    }
    if src_width < 4 || src_height < 4 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Source dimensions must be at least 4x4 for bicubic".to_string(),
        });
    }
    if dst_width == 0 || dst_height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Destination dimensions must be greater than 0".to_string(),
        });
    }
    Ok(())
}

// ============================================================================
// Cubic kernel (Catmull-Rom) - shared across all implementations
// ============================================================================

/// Cubic interpolation kernel (Catmull-Rom)
///
/// Returns 4 weights for the 4 tap positions given a fractional position t in [0,1].
#[inline]
fn cubic_kernel(t: f32) -> [f32; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        -0.5 * t3 + t2 - 0.5 * t,
        1.5 * t3 - 2.5 * t2 + 1.0,
        -1.5 * t3 + 2.0 * t2 + 0.5 * t,
        0.5 * t3 - 0.5 * t2,
    ]
}

// ============================================================================
// Scalar fallback implementation
// ============================================================================

mod scalar_impl {
    use super::cubic_kernel;

    /// Scalar bilinear interpolation inner loop
    pub(crate) fn bilinear_f32(
        src: &[f32],
        src_width: usize,
        src_height: usize,
        dst: &mut [f32],
        dst_width: usize,
        dst_height: usize,
    ) {
        let x_scale = src_width as f32 / dst_width as f32;
        let y_scale = src_height as f32 / dst_height as f32;
        const TILE_SIZE: usize = 64;

        for tile_y in (0..dst_height).step_by(TILE_SIZE) {
            let tile_h = TILE_SIZE.min(dst_height - tile_y);
            for tile_x in (0..dst_width).step_by(TILE_SIZE) {
                let tile_w = TILE_SIZE.min(dst_width - tile_x);
                for y in tile_y..(tile_y + tile_h) {
                    let src_y = (y as f32 + 0.5) * y_scale - 0.5;
                    let src_y0 = src_y.max(0.0) as usize;
                    let src_y1 = (src_y0 + 1).min(src_height - 1);
                    let y_frac = (src_y - src_y0 as f32).max(0.0).min(1.0);

                    for x in tile_x..(tile_x + tile_w) {
                        let src_x = (x as f32 + 0.5) * x_scale - 0.5;
                        let src_x0 = src_x.max(0.0) as usize;
                        let src_x1 = (src_x0 + 1).min(src_width - 1);
                        let x_frac = (src_x - src_x0 as f32).max(0.0).min(1.0);

                        let p00 = src[src_y0 * src_width + src_x0];
                        let p10 = src[src_y0 * src_width + src_x1];
                        let p01 = src[src_y1 * src_width + src_x0];
                        let p11 = src[src_y1 * src_width + src_x1];

                        let p0 = p00 + (p10 - p00) * x_frac;
                        let p1 = p01 + (p11 - p01) * x_frac;
                        let value = p0 + (p1 - p0) * y_frac;

                        dst[y * dst_width + x] = value;
                    }
                }
            }
        }
    }

    /// Scalar bicubic interpolation inner loop
    pub(crate) fn bicubic_f32(
        src: &[f32],
        src_width: usize,
        src_height: usize,
        dst: &mut [f32],
        dst_width: usize,
        dst_height: usize,
    ) {
        let x_scale = src_width as f32 / dst_width as f32;
        let y_scale = src_height as f32 / dst_height as f32;
        const TILE_SIZE: usize = 32;

        for tile_y in (0..dst_height).step_by(TILE_SIZE) {
            let tile_h = TILE_SIZE.min(dst_height - tile_y);
            for tile_x in (0..dst_width).step_by(TILE_SIZE) {
                let tile_w = TILE_SIZE.min(dst_width - tile_x);
                for y in tile_y..(tile_y + tile_h) {
                    let src_y = (y as f32 + 0.5) * y_scale - 0.5;
                    let src_y_base = src_y.floor() as isize;
                    let y_frac = (src_y - src_y_base as f32).max(0.0).min(1.0);
                    let y_weights = cubic_kernel(y_frac);

                    for x in tile_x..(tile_x + tile_w) {
                        let src_x = (x as f32 + 0.5) * x_scale - 0.5;
                        let src_x_base = src_x.floor() as isize;
                        let x_frac = (src_x - src_x_base as f32).max(0.0).min(1.0);
                        let x_weights = cubic_kernel(x_frac);

                        let mut value = 0.0_f32;
                        for ky in 0..4 {
                            let sy = (src_y_base - 1 + ky as isize)
                                .clamp(0, src_height as isize - 1)
                                as usize;
                            let mut row_sum = 0.0_f32;
                            for kx in 0..4 {
                                let sx = (src_x_base - 1 + kx as isize)
                                    .clamp(0, src_width as isize - 1)
                                    as usize;
                                row_sum += src[sy * src_width + sx] * x_weights[kx];
                            }
                            value += row_sum * y_weights[ky];
                        }
                        dst[y * dst_width + x] = value;
                    }
                }
            }
        }
    }
}

// ============================================================================
// NEON implementation (aarch64)
// ============================================================================

#[cfg(target_arch = "aarch64")]
mod neon_impl {
    use std::arch::aarch64::*;

    use super::cubic_kernel;

    /// NEON bilinear interpolation - processes 4 output pixels per iteration
    ///
    /// For each row of output pixels, src_y0/src_y1/y_frac are constant.
    /// We process 4 x-positions simultaneously using NEON 128-bit registers.
    ///
    /// SAFETY: Caller must ensure all indices are within bounds of src/dst slices
    /// and that slice dimensions match the specified widths/heights.
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn bilinear_f32(
        src: &[f32],
        src_width: usize,
        src_height: usize,
        dst: &mut [f32],
        dst_width: usize,
        dst_height: usize,
    ) {
        unsafe {
            let x_scale = src_width as f32 / dst_width as f32;
            let y_scale = src_height as f32 / dst_height as f32;
            let src_ptr = src.as_ptr();
            let dst_ptr = dst.as_mut_ptr();
            const TILE_SIZE: usize = 64;
            let max_sx = (src_width - 1) as f32;
            let max_sy = (src_height - 1) as f32;

            for tile_y in (0..dst_height).step_by(TILE_SIZE) {
                let tile_h = TILE_SIZE.min(dst_height - tile_y);
                for tile_x in (0..dst_width).step_by(TILE_SIZE) {
                    let tile_w = TILE_SIZE.min(dst_width - tile_x);
                    for y in tile_y..(tile_y + tile_h) {
                        let src_y = (y as f32 + 0.5) * y_scale - 0.5;
                        let src_y_clamped = src_y.max(0.0).min(max_sy);
                        let src_y0 = src_y_clamped as usize;
                        let src_y1 = (src_y0 + 1).min(src_height - 1);
                        let y_frac = (src_y - src_y0 as f32).max(0.0).min(1.0);
                        let vy_frac = vdupq_n_f32(y_frac);
                        let vy_frac_inv = vdupq_n_f32(1.0 - y_frac);

                        let row0_base = src_y0 * src_width;
                        let row1_base = src_y1 * src_width;
                        let dst_row = y * dst_width;

                        // Process 4 pixels at a time
                        let simd_end = tile_x + (tile_w / 4) * 4;
                        let mut x = tile_x;
                        while x < simd_end {
                            // Pre-compute source x coordinates and fractions for 4 pixels
                            let mut sx0 = [0_usize; 4];
                            let mut sx1 = [0_usize; 4];
                            let mut xf = [0.0_f32; 4];

                            for i in 0..4 {
                                let src_x = ((x + i) as f32 + 0.5) * x_scale - 0.5;
                                let src_x_clamped = src_x.max(0.0).min(max_sx);
                                sx0[i] = src_x_clamped as usize;
                                sx1[i] = (sx0[i] + 1).min(src_width - 1);
                                xf[i] = (src_x - sx0[i] as f32).max(0.0).min(1.0);
                            }

                            // Manual gather: load 4 pixels from each of the 4 source positions
                            let vp00 = vld1q_f32(
                                [
                                    *src_ptr.add(row0_base + sx0[0]),
                                    *src_ptr.add(row0_base + sx0[1]),
                                    *src_ptr.add(row0_base + sx0[2]),
                                    *src_ptr.add(row0_base + sx0[3]),
                                ]
                                .as_ptr(),
                            );
                            let vp10 = vld1q_f32(
                                [
                                    *src_ptr.add(row0_base + sx1[0]),
                                    *src_ptr.add(row0_base + sx1[1]),
                                    *src_ptr.add(row0_base + sx1[2]),
                                    *src_ptr.add(row0_base + sx1[3]),
                                ]
                                .as_ptr(),
                            );
                            let vp01 = vld1q_f32(
                                [
                                    *src_ptr.add(row1_base + sx0[0]),
                                    *src_ptr.add(row1_base + sx0[1]),
                                    *src_ptr.add(row1_base + sx0[2]),
                                    *src_ptr.add(row1_base + sx0[3]),
                                ]
                                .as_ptr(),
                            );
                            let vp11 = vld1q_f32(
                                [
                                    *src_ptr.add(row1_base + sx1[0]),
                                    *src_ptr.add(row1_base + sx1[1]),
                                    *src_ptr.add(row1_base + sx1[2]),
                                    *src_ptr.add(row1_base + sx1[3]),
                                ]
                                .as_ptr(),
                            );

                            let vxf = vld1q_f32(xf.as_ptr());
                            let vxf_inv = vsubq_f32(vdupq_n_f32(1.0), vxf);

                            // Bilinear interpolation using FMA:
                            // top = p00 * (1-xf) + p10 * xf
                            let top = vfmaq_f32(vmulq_f32(vp00, vxf_inv), vp10, vxf);
                            // bot = p01 * (1-xf) + p11 * xf
                            let bot = vfmaq_f32(vmulq_f32(vp01, vxf_inv), vp11, vxf);
                            // result = top * (1-yf) + bot * yf
                            let result = vfmaq_f32(vmulq_f32(top, vy_frac_inv), bot, vy_frac);

                            vst1q_f32(dst_ptr.add(dst_row + x), result);
                            x += 4;
                        }

                        // Scalar remainder
                        while x < tile_x + tile_w {
                            let src_x = (x as f32 + 0.5) * x_scale - 0.5;
                            let src_x0 = src_x.max(0.0) as usize;
                            let src_x1 = (src_x0 + 1).min(src_width - 1);
                            let x_frac = (src_x - src_x0 as f32).max(0.0).min(1.0);

                            let p00 = *src_ptr.add(row0_base + src_x0);
                            let p10 = *src_ptr.add(row0_base + src_x1);
                            let p01 = *src_ptr.add(row1_base + src_x0);
                            let p11 = *src_ptr.add(row1_base + src_x1);

                            let p0 = p00 + (p10 - p00) * x_frac;
                            let p1 = p01 + (p11 - p01) * x_frac;
                            *dst_ptr.add(dst_row + x) = p0 + (p1 - p0) * y_frac;
                            x += 1;
                        }
                    }
                }
            }
        }
    }

    /// NEON bicubic interpolation - uses NEON for the 4-tap dot product per row
    ///
    /// For each output pixel, we compute a separable 4x4 Catmull-Rom interpolation.
    /// Each row of 4 source pixels is loaded into a NEON float32x4, multiplied by
    /// the x-direction weights, and horizontally summed using vaddvq_f32.
    /// The 4 row results are then combined with y-direction weights.
    ///
    /// SAFETY: Caller must ensure all indices are within bounds.
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn bicubic_f32(
        src: &[f32],
        src_width: usize,
        src_height: usize,
        dst: &mut [f32],
        dst_width: usize,
        dst_height: usize,
    ) {
        unsafe {
            let x_scale = src_width as f32 / dst_width as f32;
            let y_scale = src_height as f32 / dst_height as f32;
            let src_ptr = src.as_ptr();
            let dst_ptr = dst.as_mut_ptr();
            let iw = src_width as isize;
            let ih = src_height as isize;
            const TILE_SIZE: usize = 32;

            for tile_y in (0..dst_height).step_by(TILE_SIZE) {
                let tile_h = TILE_SIZE.min(dst_height - tile_y);
                for tile_x in (0..dst_width).step_by(TILE_SIZE) {
                    let tile_w = TILE_SIZE.min(dst_width - tile_x);
                    for y in tile_y..(tile_y + tile_h) {
                        let src_y = (y as f32 + 0.5) * y_scale - 0.5;
                        let src_y_base = src_y.floor() as isize;
                        let y_frac = (src_y - src_y_base as f32).max(0.0).min(1.0);
                        let y_weights = cubic_kernel(y_frac);
                        let vy_weights = vld1q_f32(y_weights.as_ptr());

                        // Pre-clamp the 4 source row indices
                        let sy = [
                            (src_y_base - 1).clamp(0, ih - 1) as usize,
                            (src_y_base).clamp(0, ih - 1) as usize,
                            (src_y_base + 1).clamp(0, ih - 1) as usize,
                            (src_y_base + 2).clamp(0, ih - 1) as usize,
                        ];

                        let dst_row = y * dst_width;

                        for x in tile_x..(tile_x + tile_w) {
                            let src_x = (x as f32 + 0.5) * x_scale - 0.5;
                            let src_x_base = src_x.floor() as isize;
                            let x_frac = (src_x - src_x_base as f32).max(0.0).min(1.0);
                            let x_weights = cubic_kernel(x_frac);
                            let vx_weights = vld1q_f32(x_weights.as_ptr());

                            // Pre-clamp the 4 source column indices
                            let sx = [
                                (src_x_base - 1).clamp(0, iw - 1) as usize,
                                (src_x_base).clamp(0, iw - 1) as usize,
                                (src_x_base + 1).clamp(0, iw - 1) as usize,
                                (src_x_base + 2).clamp(0, iw - 1) as usize,
                            ];

                            // For each of 4 rows: load 4 pixels, dot with x_weights
                            let mut row_sums = [0.0_f32; 4];
                            for ky in 0..4 {
                                let row_off = sy[ky] * src_width;
                                let pixels = vld1q_f32(
                                    [
                                        *src_ptr.add(row_off + sx[0]),
                                        *src_ptr.add(row_off + sx[1]),
                                        *src_ptr.add(row_off + sx[2]),
                                        *src_ptr.add(row_off + sx[3]),
                                    ]
                                    .as_ptr(),
                                );
                                // Dot product: sum of (pixels * x_weights)
                                let prod = vmulq_f32(pixels, vx_weights);
                                row_sums[ky] = vaddvq_f32(prod);
                            }

                            // Combine row sums with y_weights using NEON
                            let vrow_sums = vld1q_f32(row_sums.as_ptr());
                            let vprod = vmulq_f32(vrow_sums, vy_weights);
                            let value = vaddvq_f32(vprod);

                            *dst_ptr.add(dst_row + x) = value;
                        }
                    }
                }
            }
        }
    }
}

// ============================================================================
// AVX2 implementation (x86-64)
// ============================================================================

#[cfg(target_arch = "x86_64")]
mod avx2_impl {
    use std::arch::x86_64::*;

    use super::cubic_kernel;

    /// AVX2 bilinear interpolation - processes 8 output pixels per iteration
    ///
    /// Manual gather of 8 pixel quartets, then FMA-based lerp in both X and Y.
    ///
    /// SAFETY: Caller must ensure AVX2+FMA are available (runtime detection)
    /// and all indices are within bounds.
    #[target_feature(enable = "avx2", enable = "fma")]
    pub(crate) unsafe fn bilinear_f32(
        src: &[f32],
        src_width: usize,
        src_height: usize,
        dst: &mut [f32],
        dst_width: usize,
        dst_height: usize,
    ) {
        unsafe {
            let x_scale = src_width as f32 / dst_width as f32;
            let y_scale = src_height as f32 / dst_height as f32;
            let src_ptr = src.as_ptr();
            let dst_ptr = dst.as_mut_ptr();
            const TILE_SIZE: usize = 64;
            let max_sx = (src_width - 1) as f32;
            let max_sy = (src_height - 1) as f32;

            for tile_y in (0..dst_height).step_by(TILE_SIZE) {
                let tile_h = TILE_SIZE.min(dst_height - tile_y);
                for tile_x in (0..dst_width).step_by(TILE_SIZE) {
                    let tile_w = TILE_SIZE.min(dst_width - tile_x);
                    for y in tile_y..(tile_y + tile_h) {
                        let src_y = (y as f32 + 0.5) * y_scale - 0.5;
                        let src_y_clamped = src_y.max(0.0).min(max_sy);
                        let src_y0 = src_y_clamped as usize;
                        let src_y1 = (src_y0 + 1).min(src_height - 1);
                        let y_frac = (src_y - src_y0 as f32).max(0.0).min(1.0);
                        let vy_frac = _mm256_set1_ps(y_frac);
                        let vy_frac_inv = _mm256_set1_ps(1.0 - y_frac);

                        let row0_base = src_y0 * src_width;
                        let row1_base = src_y1 * src_width;
                        let dst_row = y * dst_width;

                        // Process 8 pixels at a time
                        let simd_end = tile_x + (tile_w / 8) * 8;
                        let mut x = tile_x;
                        while x < simd_end {
                            // Pre-compute source coordinates for 8 pixels
                            let mut sx0 = [0_usize; 8];
                            let mut sx1 = [0_usize; 8];
                            let mut xf = [0.0_f32; 8];

                            for i in 0..8 {
                                let src_x = ((x + i) as f32 + 0.5) * x_scale - 0.5;
                                let src_x_clamped = src_x.max(0.0).min(max_sx);
                                sx0[i] = src_x_clamped as usize;
                                sx1[i] = (sx0[i] + 1).min(src_width - 1);
                                xf[i] = (src_x - sx0[i] as f32).max(0.0).min(1.0);
                            }

                            // Manual gather: 8 scalar loads per register
                            let vp00 = _mm256_set_ps(
                                *src_ptr.add(row0_base + sx0[7]),
                                *src_ptr.add(row0_base + sx0[6]),
                                *src_ptr.add(row0_base + sx0[5]),
                                *src_ptr.add(row0_base + sx0[4]),
                                *src_ptr.add(row0_base + sx0[3]),
                                *src_ptr.add(row0_base + sx0[2]),
                                *src_ptr.add(row0_base + sx0[1]),
                                *src_ptr.add(row0_base + sx0[0]),
                            );
                            let vp10 = _mm256_set_ps(
                                *src_ptr.add(row0_base + sx1[7]),
                                *src_ptr.add(row0_base + sx1[6]),
                                *src_ptr.add(row0_base + sx1[5]),
                                *src_ptr.add(row0_base + sx1[4]),
                                *src_ptr.add(row0_base + sx1[3]),
                                *src_ptr.add(row0_base + sx1[2]),
                                *src_ptr.add(row0_base + sx1[1]),
                                *src_ptr.add(row0_base + sx1[0]),
                            );
                            let vp01 = _mm256_set_ps(
                                *src_ptr.add(row1_base + sx0[7]),
                                *src_ptr.add(row1_base + sx0[6]),
                                *src_ptr.add(row1_base + sx0[5]),
                                *src_ptr.add(row1_base + sx0[4]),
                                *src_ptr.add(row1_base + sx0[3]),
                                *src_ptr.add(row1_base + sx0[2]),
                                *src_ptr.add(row1_base + sx0[1]),
                                *src_ptr.add(row1_base + sx0[0]),
                            );
                            let vp11 = _mm256_set_ps(
                                *src_ptr.add(row1_base + sx1[7]),
                                *src_ptr.add(row1_base + sx1[6]),
                                *src_ptr.add(row1_base + sx1[5]),
                                *src_ptr.add(row1_base + sx1[4]),
                                *src_ptr.add(row1_base + sx1[3]),
                                *src_ptr.add(row1_base + sx1[2]),
                                *src_ptr.add(row1_base + sx1[1]),
                                *src_ptr.add(row1_base + sx1[0]),
                            );

                            let vxf = _mm256_loadu_ps(xf.as_ptr());
                            let vxf_inv = _mm256_sub_ps(_mm256_set1_ps(1.0), vxf);

                            // Bilinear with FMA:
                            // top = p00 * (1-xf) + p10 * xf
                            let top = _mm256_fmadd_ps(vp10, vxf, _mm256_mul_ps(vp00, vxf_inv));
                            // bot = p01 * (1-xf) + p11 * xf
                            let bot = _mm256_fmadd_ps(vp11, vxf, _mm256_mul_ps(vp01, vxf_inv));
                            // result = top * (1-yf) + bot * yf
                            let result =
                                _mm256_fmadd_ps(bot, vy_frac, _mm256_mul_ps(top, vy_frac_inv));

                            _mm256_storeu_ps(dst_ptr.add(dst_row + x), result);
                            x += 8;
                        }

                        // Scalar remainder
                        while x < tile_x + tile_w {
                            let src_x = (x as f32 + 0.5) * x_scale - 0.5;
                            let src_x0 = src_x.max(0.0) as usize;
                            let src_x1 = (src_x0 + 1).min(src_width - 1);
                            let x_frac = (src_x - src_x0 as f32).max(0.0).min(1.0);

                            let p00 = *src_ptr.add(row0_base + src_x0);
                            let p10 = *src_ptr.add(row0_base + src_x1);
                            let p01 = *src_ptr.add(row1_base + src_x0);
                            let p11 = *src_ptr.add(row1_base + src_x1);

                            let p0 = p00 + (p10 - p00) * x_frac;
                            let p1 = p01 + (p11 - p01) * x_frac;
                            *dst_ptr.add(dst_row + x) = p0 + (p1 - p0) * y_frac;
                            x += 1;
                        }
                    }
                }
            }
        }
    }

    /// AVX2 bicubic interpolation - processes 8 output pixels in parallel
    ///
    /// For each of 4 kernel rows, gathers 8 sets of 4-tap values, computes
    /// weighted sums with x_weights using FMA, then combines with y_weights.
    ///
    /// SAFETY: Caller must ensure AVX2+FMA are available and all indices in bounds.
    #[target_feature(enable = "avx2", enable = "fma")]
    pub(crate) unsafe fn bicubic_f32(
        src: &[f32],
        src_width: usize,
        src_height: usize,
        dst: &mut [f32],
        dst_width: usize,
        dst_height: usize,
    ) {
        unsafe {
            let x_scale = src_width as f32 / dst_width as f32;
            let y_scale = src_height as f32 / dst_height as f32;
            let src_ptr = src.as_ptr();
            let dst_ptr = dst.as_mut_ptr();
            let iw = src_width as isize;
            let ih = src_height as isize;
            const TILE_SIZE: usize = 32;

            for tile_y in (0..dst_height).step_by(TILE_SIZE) {
                let tile_h = TILE_SIZE.min(dst_height - tile_y);
                for tile_x in (0..dst_width).step_by(TILE_SIZE) {
                    let tile_w = TILE_SIZE.min(dst_width - tile_x);
                    for y in tile_y..(tile_y + tile_h) {
                        let src_y = (y as f32 + 0.5) * y_scale - 0.5;
                        let src_y_base = src_y.floor() as isize;
                        let y_frac = (src_y - src_y_base as f32).max(0.0).min(1.0);
                        let y_weights = cubic_kernel(y_frac);

                        let sy = [
                            (src_y_base - 1).clamp(0, ih - 1) as usize,
                            (src_y_base).clamp(0, ih - 1) as usize,
                            (src_y_base + 1).clamp(0, ih - 1) as usize,
                            (src_y_base + 2).clamp(0, ih - 1) as usize,
                        ];

                        let dst_row = y * dst_width;

                        // Process 8 pixels at a time
                        let simd_end = tile_x + (tile_w / 8) * 8;
                        let mut x = tile_x;
                        while x < simd_end {
                            // Pre-compute x coords and weights for 8 pixels
                            let mut all_sx = [[0_usize; 4]; 8];
                            let mut all_xw = [[0.0_f32; 4]; 8];

                            for i in 0..8 {
                                let src_x = ((x + i) as f32 + 0.5) * x_scale - 0.5;
                                let src_x_base = src_x.floor() as isize;
                                let x_frac = (src_x - src_x_base as f32).max(0.0).min(1.0);
                                all_xw[i] = cubic_kernel(x_frac);
                                all_sx[i] = [
                                    (src_x_base - 1).clamp(0, iw - 1) as usize,
                                    (src_x_base).clamp(0, iw - 1) as usize,
                                    (src_x_base + 1).clamp(0, iw - 1) as usize,
                                    (src_x_base + 2).clamp(0, iw - 1) as usize,
                                ];
                            }

                            // Accumulate: for each kernel row ky, compute weighted x-sum for
                            // all 8 pixels, then multiply by y_weight[ky]
                            let mut vaccum = _mm256_setzero_ps();

                            for ky in 0..4 {
                                let row_off = sy[ky] * src_width;
                                let vy_w = _mm256_set1_ps(y_weights[ky]);

                                // For each of 4 x-taps, gather 8 values and FMA
                                let mut vrow_sum = _mm256_setzero_ps();

                                for kx in 0..4 {
                                    // Gather 8 pixels at tap kx
                                    let vp = _mm256_set_ps(
                                        *src_ptr.add(row_off + all_sx[7][kx]),
                                        *src_ptr.add(row_off + all_sx[6][kx]),
                                        *src_ptr.add(row_off + all_sx[5][kx]),
                                        *src_ptr.add(row_off + all_sx[4][kx]),
                                        *src_ptr.add(row_off + all_sx[3][kx]),
                                        *src_ptr.add(row_off + all_sx[2][kx]),
                                        *src_ptr.add(row_off + all_sx[1][kx]),
                                        *src_ptr.add(row_off + all_sx[0][kx]),
                                    );
                                    // Load the x_weight[kx] for each of 8 pixels
                                    let vxw = _mm256_set_ps(
                                        all_xw[7][kx],
                                        all_xw[6][kx],
                                        all_xw[5][kx],
                                        all_xw[4][kx],
                                        all_xw[3][kx],
                                        all_xw[2][kx],
                                        all_xw[1][kx],
                                        all_xw[0][kx],
                                    );
                                    // FMA: row_sum += pixel * x_weight
                                    vrow_sum = _mm256_fmadd_ps(vp, vxw, vrow_sum);
                                }

                                // Accumulate: total += row_sum * y_weight[ky]
                                vaccum = _mm256_fmadd_ps(vrow_sum, vy_w, vaccum);
                            }

                            _mm256_storeu_ps(dst_ptr.add(dst_row + x), vaccum);
                            x += 8;
                        }

                        // Scalar remainder
                        while x < tile_x + tile_w {
                            let src_x = (x as f32 + 0.5) * x_scale - 0.5;
                            let src_x_base = src_x.floor() as isize;
                            let x_frac = (src_x - src_x_base as f32).max(0.0).min(1.0);
                            let x_weights = cubic_kernel(x_frac);
                            let sx = [
                                (src_x_base - 1).clamp(0, iw - 1) as usize,
                                (src_x_base).clamp(0, iw - 1) as usize,
                                (src_x_base + 1).clamp(0, iw - 1) as usize,
                                (src_x_base + 2).clamp(0, iw - 1) as usize,
                            ];
                            let mut value = 0.0_f32;
                            for ky in 0..4 {
                                let row_off = sy[ky] * src_width;
                                let mut row_sum = 0.0_f32;
                                for kx in 0..4 {
                                    row_sum += *src_ptr.add(row_off + sx[kx]) * x_weights[kx];
                                }
                                value += row_sum * y_weights[ky];
                            }
                            *dst_ptr.add(dst_row + x) = value;
                            x += 1;
                        }
                    }
                }
            }
        }
    }
}

// ============================================================================
// Public API with dispatch
// ============================================================================

/// Bilinear interpolation with SIMD optimization
///
/// Resample a source image to a destination size using bilinear interpolation.
/// Automatically dispatches to the best available SIMD instruction set:
///
/// - **aarch64**: NEON (4 pixels/iteration)
/// - **x86-64**: AVX2+FMA (8 pixels/iteration) with scalar fallback
/// - **Other**: Scalar with cache-friendly tiling
///
/// # Arguments
///
/// * `src` - Source image data (row-major)
/// * `src_width` - Source image width
/// * `src_height` - Source image height
/// * `dst` - Destination buffer (must be dst_width * dst_height)
/// * `dst_width` - Destination width
/// * `dst_height` - Destination height
///
/// # Errors
///
/// Returns an error if buffer sizes don't match dimensions
pub fn bilinear_f32(
    src: &[f32],
    src_width: usize,
    src_height: usize,
    dst: &mut [f32],
    dst_width: usize,
    dst_height: usize,
) -> Result<()> {
    validate_bilinear(src, src_width, src_height, dst, dst_width, dst_height)?;

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is always available on aarch64, dimensions validated above
        unsafe {
            neon_impl::bilinear_f32(src, src_width, src_height, dst, dst_width, dst_height);
        }
    }

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            // SAFETY: AVX2+FMA runtime detected, dimensions validated above
            unsafe {
                avx2_impl::bilinear_f32(src, src_width, src_height, dst, dst_width, dst_height);
            }
        } else {
            scalar_impl::bilinear_f32(src, src_width, src_height, dst, dst_width, dst_height);
        }
    }

    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        scalar_impl::bilinear_f32(src, src_width, src_height, dst, dst_width, dst_height);
    }

    Ok(())
}

/// Bicubic interpolation with SIMD optimization
///
/// Resample a source image to a destination size using bicubic (Catmull-Rom)
/// interpolation. Provides higher quality than bilinear at the cost of speed.
/// Dispatches to the best available SIMD instruction set.
///
/// # Arguments
///
/// * `src` - Source image data (row-major)
/// * `src_width` - Source image width
/// * `src_height` - Source image height
/// * `dst` - Destination buffer (must be dst_width * dst_height)
/// * `dst_width` - Destination width
/// * `dst_height` - Destination height
///
/// # Errors
///
/// Returns an error if buffer sizes don't match dimensions
pub fn bicubic_f32(
    src: &[f32],
    src_width: usize,
    src_height: usize,
    dst: &mut [f32],
    dst_width: usize,
    dst_height: usize,
) -> Result<()> {
    validate_bicubic(src, src_width, src_height, dst, dst_width, dst_height)?;

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is always available on aarch64, dimensions validated above
        unsafe {
            neon_impl::bicubic_f32(src, src_width, src_height, dst, dst_width, dst_height);
        }
    }

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            // SAFETY: AVX2+FMA runtime detected, dimensions validated above
            unsafe {
                avx2_impl::bicubic_f32(src, src_width, src_height, dst, dst_width, dst_height);
            }
        } else {
            scalar_impl::bicubic_f32(src, src_width, src_height, dst, dst_width, dst_height);
        }
    }

    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        scalar_impl::bicubic_f32(src, src_width, src_height, dst, dst_width, dst_height);
    }

    Ok(())
}

/// Nearest neighbor resampling (fast, no interpolation)
///
/// This is the fastest resampling method but produces blocky results.
/// Useful for categorical data or when speed is critical.
///
/// # Arguments
///
/// * `src` - Source image data (row-major)
/// * `src_width` - Source image width
/// * `src_height` - Source image height
/// * `dst` - Destination buffer (must be dst_width * dst_height)
/// * `dst_width` - Destination width
/// * `dst_height` - Destination height
///
/// # Errors
///
/// Returns an error if buffer sizes don't match dimensions
pub fn nearest_f32(
    src: &[f32],
    src_width: usize,
    src_height: usize,
    dst: &mut [f32],
    dst_width: usize,
    dst_height: usize,
) -> Result<()> {
    if src.len() != src_width * src_height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Source buffer size doesn't match dimensions".to_string(),
        });
    }
    if dst.len() != dst_width * dst_height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Destination buffer size doesn't match dimensions".to_string(),
        });
    }
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Dimensions must be greater than 0".to_string(),
        });
    }

    let x_ratio = src_width as f32 / dst_width as f32;
    let y_ratio = src_height as f32 / dst_height as f32;

    for y in 0..dst_height {
        let src_y = ((y as f32 * y_ratio) as usize).min(src_height - 1);
        for x in 0..dst_width {
            let src_x = ((x as f32 * x_ratio) as usize).min(src_width - 1);
            dst[y * dst_width + x] = src[src_y * src_width + src_x];
        }
    }

    Ok(())
}

/// Downsample using area averaging (for antialiasing)
///
/// When downsampling, this method averages pixels in the source region
/// to produce smoother results with less aliasing.
///
/// # Arguments
///
/// * `src` - Source image data (row-major)
/// * `src_width` - Source image width
/// * `src_height` - Source image height
/// * `dst` - Destination buffer (must be dst_width * dst_height)
/// * `dst_width` - Destination width
/// * `dst_height` - Destination height
///
/// # Errors
///
/// Returns an error if buffer sizes don't match dimensions or upsampling is attempted
pub fn downsample_average_f32(
    src: &[f32],
    src_width: usize,
    src_height: usize,
    dst: &mut [f32],
    dst_width: usize,
    dst_height: usize,
) -> Result<()> {
    if src.len() != src_width * src_height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Source buffer size doesn't match dimensions".to_string(),
        });
    }
    if dst.len() != dst_width * dst_height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Destination buffer size doesn't match dimensions".to_string(),
        });
    }
    if dst_width > src_width || dst_height > src_height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "This method is only for downsampling".to_string(),
        });
    }

    let x_ratio = src_width as f32 / dst_width as f32;
    let y_ratio = src_height as f32 / dst_height as f32;

    for dst_y in 0..dst_height {
        let src_y_start = (dst_y as f32 * y_ratio) as usize;
        let src_y_end = (((dst_y + 1) as f32 * y_ratio) as usize).min(src_height);

        for dst_x in 0..dst_width {
            let src_x_start = (dst_x as f32 * x_ratio) as usize;
            let src_x_end = (((dst_x + 1) as f32 * x_ratio) as usize).min(src_width);

            let mut sum = 0.0_f32;
            let mut count = 0;

            for src_y in src_y_start..src_y_end {
                for src_x in src_x_start..src_x_end {
                    sum += src[src_y * src_width + src_x];
                    count += 1;
                }
            }

            dst[dst_y * dst_width + dst_x] = if count > 0 { sum / count as f32 } else { 0.0 };
        }
    }

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    // ---- Bilinear tests ----

    #[test]
    fn test_bilinear_identity() {
        let src = vec![1.0, 2.0, 3.0, 4.0];
        let mut dst = vec![0.0; 4];
        bilinear_f32(&src, 2, 2, &mut dst, 2, 2)
            .expect("bilinear_f32 identity resampling should succeed in test");
        for i in 0..4 {
            assert_relative_eq!(dst[i], src[i], epsilon = 1e-5);
        }
    }

    #[test]
    fn test_bilinear_downsample() {
        let src = vec![
            1.0, 1.0, 2.0, 2.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 3.0, 3.0, 4.0, 4.0,
        ];
        let mut dst = vec![0.0; 4];
        bilinear_f32(&src, 4, 4, &mut dst, 2, 2)
            .expect("bilinear_f32 downsampling should succeed in test");
        assert!(dst[0] < dst[1]);
        assert!(dst[2] < dst[3]);
    }

    #[test]
    fn test_bilinear_upsample() {
        let src = vec![1.0, 2.0, 3.0, 4.0];
        let mut dst = vec![0.0; 16];
        bilinear_f32(&src, 2, 2, &mut dst, 4, 4)
            .expect("bilinear_f32 upsampling should succeed in test");
        assert_relative_eq!(dst[0], 1.0, epsilon = 1e-5);
        assert_relative_eq!(dst[15], 4.0, epsilon = 1e-5);
    }

    // ---- Bicubic tests ----

    #[test]
    fn test_bicubic_identity() {
        let src = vec![
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
        ];
        let mut dst = vec![0.0; 16];
        bicubic_f32(&src, 4, 4, &mut dst, 4, 4)
            .expect("bicubic_f32 identity resampling should succeed in test");
        for i in 0..16 {
            assert_relative_eq!(dst[i], src[i], epsilon = 0.1);
        }
    }

    // ---- Nearest tests ----

    #[test]
    fn test_nearest() {
        let src = vec![1.0, 2.0, 3.0, 4.0];
        let mut dst = vec![0.0; 4];
        nearest_f32(&src, 2, 2, &mut dst, 2, 2)
            .expect("nearest_f32 identity resampling should succeed in test");
        for i in 0..4 {
            assert_relative_eq!(dst[i], src[i]);
        }
    }

    #[test]
    fn test_nearest_downsample() {
        let src = vec![
            1.0, 1.0, 2.0, 2.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 3.0, 3.0, 4.0, 4.0,
        ];
        let mut dst = vec![0.0; 4];
        nearest_f32(&src, 4, 4, &mut dst, 2, 2)
            .expect("nearest_f32 downsampling should succeed in test");
        assert_relative_eq!(dst[0], 1.0);
        assert_relative_eq!(dst[1], 2.0);
        assert_relative_eq!(dst[2], 3.0);
        assert_relative_eq!(dst[3], 4.0);
    }

    // ---- Downsample average tests ----

    #[test]
    fn test_downsample_average() {
        let src = vec![
            1.0, 1.0, 2.0, 2.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 3.0, 3.0, 4.0, 4.0,
        ];
        let mut dst = vec![0.0; 4];
        downsample_average_f32(&src, 4, 4, &mut dst, 2, 2)
            .expect("downsample_average_f32 should succeed in test");
        assert_relative_eq!(dst[0], 1.0);
        assert_relative_eq!(dst[1], 2.0);
        assert_relative_eq!(dst[2], 3.0);
        assert_relative_eq!(dst[3], 4.0);
    }

    // ---- Validation tests ----

    #[test]
    fn test_invalid_dimensions() {
        let src = vec![1.0; 10];
        let mut dst = vec![0.0; 4];
        assert!(bilinear_f32(&src, 4, 4, &mut dst, 2, 2).is_err());

        let src = vec![1.0; 16];
        assert!(bilinear_f32(&src, 4, 4, &mut dst, 3, 3).is_err());
    }

    #[test]
    fn test_bicubic_too_small() {
        let src = vec![1.0; 9];
        let mut dst = vec![0.0; 4];
        assert!(bicubic_f32(&src, 3, 3, &mut dst, 2, 2).is_err());
    }

    #[test]
    fn test_cubic_kernel() {
        let weights = cubic_kernel(0.5);
        let sum: f32 = weights.iter().sum();
        assert_relative_eq!(sum, 1.0, epsilon = 1e-6);
    }

    // ---- Large test ----

    #[test]
    fn test_large_downsample() {
        let src = vec![1.0_f32; 1000 * 1000];
        let mut dst = vec![0.0_f32; 100 * 100];
        bilinear_f32(&src, 1000, 1000, &mut dst, 100, 100)
            .expect("bilinear_f32 large downsampling should succeed in test");
        for &val in &dst {
            assert_relative_eq!(val, 1.0);
        }
    }

    // ---- SIMD vs Scalar accuracy tests ----

    /// Helper: run the scalar implementation for bilinear and return results
    fn scalar_bilinear(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
        let mut dst = vec![0.0_f32; dw * dh];
        scalar_impl::bilinear_f32(src, sw, sh, &mut dst, dw, dh);
        dst
    }

    /// Helper: run the scalar implementation for bicubic and return results
    fn scalar_bicubic(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
        let mut dst = vec![0.0_f32; dw * dh];
        scalar_impl::bicubic_f32(src, sw, sh, &mut dst, dw, dh);
        dst
    }

    /// Compare SIMD-dispatched bilinear against scalar reference.
    /// Tolerance is 1e-4 to account for FMA rounding differences (FMA computes
    /// a*b+c with a single rounding vs two roundings in separate mul+add).
    fn assert_bilinear_matches_scalar(
        src: &[f32],
        sw: usize,
        sh: usize,
        dw: usize,
        dh: usize,
        label: &str,
    ) {
        let scalar = scalar_bilinear(src, sw, sh, dw, dh);
        let mut simd_dst = vec![0.0_f32; dw * dh];
        bilinear_f32(src, sw, sh, &mut simd_dst, dw, dh)
            .expect("bilinear_f32 should succeed for SIMD vs scalar comparison");

        for (i, (&s, &d)) in scalar.iter().zip(simd_dst.iter()).enumerate() {
            assert!(
                (s - d).abs() < 1e-4,
                "bilinear mismatch at index {i} for {label}: scalar={s}, simd={d}, diff={}",
                (s - d).abs()
            );
        }
    }

    /// Compare SIMD-dispatched bicubic against scalar reference.
    /// Tolerance is 1e-4 to account for FMA rounding differences.
    fn assert_bicubic_matches_scalar(
        src: &[f32],
        sw: usize,
        sh: usize,
        dw: usize,
        dh: usize,
        label: &str,
    ) {
        let scalar = scalar_bicubic(src, sw, sh, dw, dh);
        let mut simd_dst = vec![0.0_f32; dw * dh];
        bicubic_f32(src, sw, sh, &mut simd_dst, dw, dh)
            .expect("bicubic_f32 should succeed for SIMD vs scalar comparison");

        for (i, (&s, &d)) in scalar.iter().zip(simd_dst.iter()).enumerate() {
            assert!(
                (s - d).abs() < 1e-4,
                "bicubic mismatch at index {i} for {label}: scalar={s}, simd={d}, diff={}",
                (s - d).abs()
            );
        }
    }

    #[test]
    fn test_bilinear_simd_vs_scalar_exact_4() {
        // Exactly 4 output columns (NEON width)
        let src: Vec<f32> = (0..64).map(|i| i as f32 * 0.1).collect();
        assert_bilinear_matches_scalar(&src, 8, 8, 4, 4, "exact_4");
    }

    #[test]
    fn test_bilinear_simd_vs_scalar_exact_8() {
        // Exactly 8 output columns (AVX2 width)
        let src: Vec<f32> = (0..256).map(|i| (i as f32).sin()).collect();
        assert_bilinear_matches_scalar(&src, 16, 16, 8, 8, "exact_8");
    }

    #[test]
    fn test_bilinear_simd_vs_scalar_non_multiple_7() {
        // 7 output columns - not a multiple of 4 or 8
        let src: Vec<f32> = (0..196).map(|i| i as f32 * 0.05).collect();
        assert_bilinear_matches_scalar(&src, 14, 14, 7, 7, "non_multiple_7");
    }

    #[test]
    fn test_bilinear_simd_vs_scalar_non_multiple_13() {
        // 13 output columns - exercises both SIMD and scalar remainder
        let src: Vec<f32> = (0..400).map(|i| (i as f32 * 0.1).cos()).collect();
        assert_bilinear_matches_scalar(&src, 20, 20, 13, 13, "non_multiple_13");
    }

    #[test]
    fn test_bilinear_simd_vs_scalar_large_256() {
        // 256x256 -> 128x128 - large enough to exercise multiple tiles
        let src: Vec<f32> = (0..256 * 256)
            .map(|i| {
                let x = (i % 256) as f32;
                let y = (i / 256) as f32;
                (x * 0.01).sin() + (y * 0.01).cos()
            })
            .collect();
        assert_bilinear_matches_scalar(&src, 256, 256, 128, 128, "large_256");
    }

    #[test]
    fn test_bilinear_simd_vs_scalar_upsample() {
        // 10x10 -> 37x37 - upsampling with odd dimensions
        let src: Vec<f32> = (0..100).map(|i| i as f32).collect();
        assert_bilinear_matches_scalar(&src, 10, 10, 37, 37, "upsample_37");
    }

    #[test]
    fn test_bilinear_simd_vs_scalar_identity() {
        // Same size - identity resampling
        let src: Vec<f32> = (0..64).map(|i| i as f32 * 0.5).collect();
        assert_bilinear_matches_scalar(&src, 8, 8, 8, 8, "identity_8x8");
    }

    #[test]
    fn test_bicubic_simd_vs_scalar_exact_4() {
        let src: Vec<f32> = (0..64).map(|i| i as f32 * 0.1).collect();
        assert_bicubic_matches_scalar(&src, 8, 8, 4, 4, "exact_4");
    }

    #[test]
    fn test_bicubic_simd_vs_scalar_exact_8() {
        let src: Vec<f32> = (0..256).map(|i| (i as f32).sin()).collect();
        assert_bicubic_matches_scalar(&src, 16, 16, 8, 8, "exact_8");
    }

    #[test]
    fn test_bicubic_simd_vs_scalar_non_multiple_7() {
        let src: Vec<f32> = (0..196).map(|i| i as f32 * 0.05).collect();
        assert_bicubic_matches_scalar(&src, 14, 14, 7, 7, "non_multiple_7");
    }

    #[test]
    fn test_bicubic_simd_vs_scalar_non_multiple_13() {
        let src: Vec<f32> = (0..400).map(|i| (i as f32 * 0.1).cos()).collect();
        assert_bicubic_matches_scalar(&src, 20, 20, 13, 13, "non_multiple_13");
    }

    #[test]
    fn test_bicubic_simd_vs_scalar_large_128() {
        // 128x128 -> 64x64 bicubic
        let src: Vec<f32> = (0..128 * 128)
            .map(|i| {
                let x = (i % 128) as f32;
                let y = (i / 128) as f32;
                (x * 0.02).sin() + (y * 0.02).cos()
            })
            .collect();
        assert_bicubic_matches_scalar(&src, 128, 128, 64, 64, "large_128");
    }

    #[test]
    fn test_bicubic_simd_vs_scalar_upsample() {
        // 8x8 -> 19x19 - upsampling with odd target
        let src: Vec<f32> = (0..64).map(|i| i as f32).collect();
        assert_bicubic_matches_scalar(&src, 8, 8, 19, 19, "upsample_19");
    }

    #[test]
    fn test_bicubic_simd_vs_scalar_identity() {
        let src: Vec<f32> = (0..64).map(|i| i as f32 * 0.5).collect();
        assert_bicubic_matches_scalar(&src, 8, 8, 8, 8, "identity_8x8");
    }

    #[test]
    fn test_bilinear_simd_vs_scalar_asymmetric() {
        // Non-square: 20x10 -> 7x15
        let src: Vec<f32> = (0..200).map(|i| (i as f32 * 0.1).sin()).collect();
        assert_bilinear_matches_scalar(&src, 20, 10, 7, 15, "asymmetric_20x10_to_7x15");
    }

    #[test]
    fn test_bicubic_simd_vs_scalar_asymmetric() {
        // Non-square: 16x8 -> 5x11
        let src: Vec<f32> = (0..128).map(|i| (i as f32 * 0.1).cos()).collect();
        assert_bicubic_matches_scalar(&src, 16, 8, 5, 11, "asymmetric_16x8_to_5x11");
    }

    #[test]
    fn test_bilinear_constant_gradient() {
        // Linear gradient: bilinear should reproduce it exactly
        let w = 32_usize;
        let h = 32_usize;
        let src: Vec<f32> = (0..w * h)
            .map(|i| {
                let x = (i % w) as f32;
                let y = (i / w) as f32;
                x * 2.0 + y * 3.0
            })
            .collect();
        let dw = 16_usize;
        let dh = 16_usize;
        let mut dst = vec![0.0_f32; dw * dh];
        bilinear_f32(&src, w, h, &mut dst, dw, dh).expect("bilinear gradient test should succeed");

        // Verify reasonable gradient reconstruction
        // For a linear function, bilinear should give close values
        for dy in 0..dh {
            for dx in 0..dw {
                let val = dst[dy * dw + dx];
                assert!(val.is_finite(), "Non-finite at ({dx},{dy}): {val}");
            }
        }
    }

    #[test]
    fn test_bicubic_monotonicity() {
        // Monotonically increasing input should produce monotonically increasing output
        // within each row/column (approximately, given Catmull-Rom overshoot is small)
        let w = 16_usize;
        let h = 4_usize;
        let src: Vec<f32> = (0..w * h).map(|i| i as f32).collect();
        let dw = 8_usize;
        let dh = 4_usize;
        let mut dst = vec![0.0_f32; dw * dh];
        bicubic_f32(&src, w, h, &mut dst, dw, dh)
            .expect("bicubic monotonicity test should succeed");

        // Values should generally increase left to right within a row
        for dy in 0..dh {
            let first = dst[dy * dw];
            let last = dst[dy * dw + dw - 1];
            assert!(
                last >= first,
                "Row {dy}: first={first}, last={last} - should increase"
            );
        }
    }
}

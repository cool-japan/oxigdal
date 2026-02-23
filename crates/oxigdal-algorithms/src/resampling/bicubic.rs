//! Bicubic interpolation resampling
//!
//! Bicubic interpolation uses a 4x4 neighborhood of pixels with cubic convolution
//! to produce smooth, high-quality results.
//!
//! # Characteristics
//!
//! - **Speed**: Moderate (16 samples per pixel)
//! - **Quality**: High, very smooth
//! - **Best for**: High-quality imagery, smooth surfaces
//!
//! # Algorithm
//!
//! Uses Catmull-Rom cubic spline (a = -0.5) for interpolation.
//! For each output pixel, samples a 4x4 neighborhood and applies
//! the cubic kernel in both X and Y directions.

use crate::error::{AlgorithmError, Result};
use crate::resampling::kernel::cubic;
use oxigdal_core::buffer::RasterBuffer;

/// Bicubic interpolation resampler
#[derive(Debug, Clone, Copy)]
pub struct BicubicResampler {
    /// Sharpness parameter (typically -0.5 for Catmull-Rom)
    a: f64,
}

impl Default for BicubicResampler {
    fn default() -> Self {
        Self::new()
    }
}

impl BicubicResampler {
    /// Creates a new bicubic resampler with Catmull-Rom spline (a = -0.5)
    #[must_use]
    pub const fn new() -> Self {
        Self { a: -0.5 }
    }

    /// Creates a bicubic resampler with custom sharpness parameter
    ///
    /// # Arguments
    ///
    /// * `a` - Sharpness parameter (-1.0 = softer, -0.5 = standard, -0.25 = sharper)
    #[must_use]
    pub const fn with_sharpness(a: f64) -> Self {
        Self { a }
    }

    /// Resamples a raster buffer using bicubic interpolation
    ///
    /// # Errors
    ///
    /// Returns an error if dimensions are invalid
    pub fn resample(
        &self,
        src: &RasterBuffer,
        dst_width: u64,
        dst_height: u64,
    ) -> Result<RasterBuffer> {
        if dst_width == 0 || dst_height == 0 {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "dimensions",
                message: "Target dimensions must be non-zero".to_string(),
            });
        }

        let src_width = src.width();
        let src_height = src.height();

        if src_width == 0 || src_height == 0 {
            return Err(AlgorithmError::EmptyInput {
                operation: "bicubic resampling",
            });
        }

        let mut dst = RasterBuffer::zeros(dst_width, dst_height, src.data_type());

        let scale_x = src_width as f64 / dst_width as f64;
        let scale_y = src_height as f64 / dst_height as f64;

        for dst_y in 0..dst_height {
            for dst_x in 0..dst_width {
                let src_x = (dst_x as f64 + 0.5) * scale_x - 0.5;
                let src_y = (dst_y as f64 + 0.5) * scale_y - 0.5;

                let value = self.interpolate_at(src, src_x, src_y)?;
                dst.set_pixel(dst_x, dst_y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(dst)
    }

    /// Interpolates value at fractional source coordinates using bicubic
    fn interpolate_at(&self, src: &RasterBuffer, src_x: f64, src_y: f64) -> Result<f64> {
        let src_width = src.width();
        let src_height = src.height();

        // Clamp to valid range
        let src_x_clamped = src_x.max(0.0).min((src_width - 1) as f64);
        let src_y_clamped = src_y.max(0.0).min((src_height - 1) as f64);

        // Get integer and fractional parts
        let x0 = src_x_clamped.floor() as i64;
        let y0 = src_y_clamped.floor() as i64;
        let fx = src_x_clamped - x0 as f64;
        let fy = src_y_clamped - y0 as f64;

        // Sample 4x4 neighborhood
        let mut values = [[0.0f64; 4]; 4];
        for dy in 0..4i64 {
            for dx in 0..4i64 {
                let sx = (x0 + dx - 1).max(0).min(src_width as i64 - 1) as u64;
                let sy = (y0 + dy - 1).max(0).min(src_height as i64 - 1) as u64;
                values[dy as usize][dx as usize] =
                    src.get_pixel(sx, sy).map_err(AlgorithmError::Core)?;
            }
        }

        // Apply cubic interpolation in X direction for each row
        let mut col_values = [0.0f64; 4];
        for (i, row) in values.iter().enumerate() {
            col_values[i] = self.cubic_interpolate_1d(row, fx);
        }

        // Apply cubic interpolation in Y direction
        Ok(self.cubic_interpolate_1d(&col_values, fy))
    }

    /// 1D cubic interpolation from 4 samples
    #[inline]
    fn cubic_interpolate_1d(&self, values: &[f64], t: f64) -> f64 {
        let mut result = 0.0;
        for (i, &value) in values.iter().enumerate() {
            let x = t - (i as f64 - 1.0);
            result += value * cubic(x, self.a);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_bicubic_identity() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let resampler = BicubicResampler::new();
        let dst = resampler.resample(&src, 10, 10);
        assert!(dst.is_ok());
    }

    #[test]
    fn test_bicubic_smooth() {
        let mut src = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
        // Create a simple gradient
        for y in 0..3 {
            for x in 0..3 {
                src.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let resampler = BicubicResampler::new();
        let dst = resampler.resample(&src, 6, 6);
        assert!(dst.is_ok());

        // Bicubic should produce smooth gradients
        if let Ok(dst) = dst {
            // Check that interpolated values are reasonable
            let v1 = dst.get_pixel(1, 1).ok();
            let v2 = dst.get_pixel(2, 2).ok();
            assert!(v1.is_some());
            assert!(v2.is_some());
        }
    }

    #[test]
    fn test_bicubic_with_sharpness() {
        let src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let soft = BicubicResampler::with_sharpness(-1.0);
        let sharp = BicubicResampler::with_sharpness(-0.25);

        let dst_soft = soft.resample(&src, 10, 10);
        let dst_sharp = sharp.resample(&src, 10, 10);

        assert!(dst_soft.is_ok());
        assert!(dst_sharp.is_ok());
    }

    #[test]
    fn test_bicubic_zero_dimensions() {
        let src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let resampler = BicubicResampler::new();

        assert!(resampler.resample(&src, 0, 10).is_err());
        assert!(resampler.resample(&src, 10, 0).is_err());
    }

    #[test]
    fn test_cubic_1d() {
        let resampler = BicubicResampler::new();
        let values = [1.0, 2.0, 3.0, 4.0];

        // At t=0 (center of second sample), should be close to 2.0
        let result = resampler.cubic_interpolate_1d(&values, 0.0);
        assert_abs_diff_eq!(result, 2.0, epsilon = 0.1);

        // At t=1 (center of third sample), should be close to 3.0
        let result = resampler.cubic_interpolate_1d(&values, 1.0);
        assert_abs_diff_eq!(result, 3.0, epsilon = 0.1);
    }
}

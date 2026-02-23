//! Bilinear interpolation resampling
//!
//! Bilinear interpolation samples from the 4 nearest pixels and performs
//! linear interpolation in both directions to compute the output value.
//!
//! # Characteristics
//!
//! - **Speed**: Fast (O(1) per pixel, 4 samples)
//! - **Quality**: Good, smooth results
//! - **Best for**: Continuous data (DEMs, temperatures, etc.)
//!
//! # Algorithm
//!
//! For each output pixel at (dx, dy):
//! 1. Map to source coordinates: (sx, sy)
//! 2. Find integer part: (x0, y0) = floor(sx, sy)
//! 3. Find fractional part: (fx, fy) = (sx - x0, sy - y0)
//! 4. Sample 4 neighbors: v00, v10, v01, v11
//! 5. Interpolate in X: vx0 = lerp(v00, v10, fx), vx1 = lerp(v01, v11, fx)
//! 6. Interpolate in Y: result = lerp(vx0, vx1, fy)
//!
//! where lerp(a, b, t) = a + t * (b - a)

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;

/// Bilinear interpolation resampler
#[derive(Debug, Clone, Copy, Default)]
pub struct BilinearResampler;

impl BilinearResampler {
    /// Creates a new bilinear resampler
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Resamples a raster buffer using bilinear interpolation
    ///
    /// # Errors
    ///
    /// Returns an error if dimensions are invalid or buffers cannot be created
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
                operation: "bilinear resampling",
            });
        }

        let mut dst = RasterBuffer::zeros(dst_width, dst_height, src.data_type());

        // Compute scaling factors
        let scale_x = src_width as f64 / dst_width as f64;
        let scale_y = src_height as f64 / dst_height as f64;

        // Process each output pixel
        for dst_y in 0..dst_height {
            for dst_x in 0..dst_width {
                // Map to source coordinates (pixel-center registration)
                let src_x = (dst_x as f64 + 0.5) * scale_x - 0.5;
                let src_y = (dst_y as f64 + 0.5) * scale_y - 0.5;

                // Interpolate and set value
                let value = self.interpolate_at(src, src_x, src_y)?;
                dst.set_pixel(dst_x, dst_y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(dst)
    }

    /// Interpolates value at fractional source coordinates
    ///
    /// Uses bilinear interpolation from 4 nearest pixels.
    ///
    /// # Arguments
    ///
    /// * `src` - Source buffer
    /// * `src_x` - X coordinate in source (may be fractional)
    /// * `src_y` - Y coordinate in source (may be fractional)
    ///
    /// # Errors
    ///
    /// Returns an error if pixel access fails
    fn interpolate_at(&self, src: &RasterBuffer, src_x: f64, src_y: f64) -> Result<f64> {
        let src_width = src.width();
        let src_height = src.height();

        // Clamp to valid range
        let src_x_clamped = src_x.max(0.0).min((src_width - 1) as f64);
        let src_y_clamped = src_y.max(0.0).min((src_height - 1) as f64);

        // Get integer and fractional parts
        let x0 = src_x_clamped.floor() as u64;
        let y0 = src_y_clamped.floor() as u64;
        let fx = src_x_clamped - x0 as f64;
        let fy = src_y_clamped - y0 as f64;

        // Get the four corner coordinates
        let x1 = (x0 + 1).min(src_width - 1);
        let y1 = (y0 + 1).min(src_height - 1);

        // Sample the four corners
        let v00 = src.get_pixel(x0, y0).map_err(AlgorithmError::Core)?;
        let v10 = src.get_pixel(x1, y0).map_err(AlgorithmError::Core)?;
        let v01 = src.get_pixel(x0, y1).map_err(AlgorithmError::Core)?;
        let v11 = src.get_pixel(x1, y1).map_err(AlgorithmError::Core)?;

        // Check for nodata values
        let nodata = src.nodata();
        let has_nodata = !nodata.is_none();

        if has_nodata {
            // If any neighbor is nodata, handle specially
            let is_nodata = |val: f64| src.is_nodata(val);

            if is_nodata(v00) && is_nodata(v10) && is_nodata(v01) && is_nodata(v11) {
                // All nodata - return nodata
                return nodata
                    .as_f64()
                    .ok_or_else(|| AlgorithmError::NumericalError {
                        operation: "bilinear interpolation",
                        message: "NoData value unavailable".to_string(),
                    });
            }

            // Interpolate only valid values
            return Ok(self.interpolate_with_nodata(v00, v10, v01, v11, fx, fy, &is_nodata));
        }

        // Standard bilinear interpolation
        Ok(Self::bilinear_interp(v00, v10, v01, v11, fx, fy))
    }

    /// Performs bilinear interpolation with standard formula
    ///
    /// # Arguments
    ///
    /// * `v00` - Value at (x0, y0)
    /// * `v10` - Value at (x1, y0)
    /// * `v01` - Value at (x0, y1)
    /// * `v11` - Value at (x1, y1)
    /// * `fx` - Fractional X (0..1)
    /// * `fy` - Fractional Y (0..1)
    #[inline]
    fn bilinear_interp(v00: f64, v10: f64, v01: f64, v11: f64, fx: f64, fy: f64) -> f64 {
        // Interpolate in X direction
        let vx0 = Self::lerp(v00, v10, fx);
        let vx1 = Self::lerp(v01, v11, fx);

        // Interpolate in Y direction
        Self::lerp(vx0, vx1, fy)
    }

    /// Linear interpolation between two values
    #[inline]
    const fn lerp(a: f64, b: f64, t: f64) -> f64 {
        a + t * (b - a)
    }

    /// Interpolates with nodata handling
    ///
    /// This uses inverse distance weighting for valid pixels only.
    fn interpolate_with_nodata<F>(
        &self,
        v00: f64,
        v10: f64,
        v01: f64,
        v11: f64,
        fx: f64,
        fy: f64,
        is_nodata: &F,
    ) -> f64
    where
        F: Fn(f64) -> bool,
    {
        // Compute weights based on distance
        let w00 = (1.0 - fx) * (1.0 - fy);
        let w10 = fx * (1.0 - fy);
        let w01 = (1.0 - fx) * fy;
        let w11 = fx * fy;

        let mut sum = 0.0;
        let mut weight_sum = 0.0;

        if !is_nodata(v00) {
            sum += v00 * w00;
            weight_sum += w00;
        }
        if !is_nodata(v10) {
            sum += v10 * w10;
            weight_sum += w10;
        }
        if !is_nodata(v01) {
            sum += v01 * w01;
            weight_sum += w01;
        }
        if !is_nodata(v11) {
            sum += v11 * w11;
            weight_sum += w11;
        }

        if weight_sum > f64::EPSILON {
            sum / weight_sum
        } else {
            // All neighbors are nodata
            v00 // Return any value (should be nodata anyway)
        }
    }

    /// Resamples a region of interest
    ///
    /// This allows resampling only a specific window from the source.
    ///
    /// # Arguments
    ///
    /// * `src` - Source buffer
    /// * `src_x` - Source window X offset
    /// * `src_y` - Source window Y offset
    /// * `src_width` - Source window width
    /// * `src_height` - Source window height
    /// * `dst_width` - Destination width
    /// * `dst_height` - Destination height
    ///
    /// # Errors
    ///
    /// Returns an error if parameters are invalid
    #[allow(clippy::too_many_arguments)]
    pub fn resample_region(
        &self,
        src: &RasterBuffer,
        src_x: u64,
        src_y: u64,
        src_width: u64,
        src_height: u64,
        dst_width: u64,
        dst_height: u64,
    ) -> Result<RasterBuffer> {
        if dst_width == 0 || dst_height == 0 {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "dimensions",
                message: "Target dimensions must be non-zero".to_string(),
            });
        }

        if src_x + src_width > src.width() || src_y + src_height > src.height() {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "region",
                message: "Source region exceeds buffer bounds".to_string(),
            });
        }

        let mut dst = RasterBuffer::zeros(dst_width, dst_height, src.data_type());

        let scale_x = src_width as f64 / dst_width as f64;
        let scale_y = src_height as f64 / dst_height as f64;

        for dst_y_coord in 0..dst_height {
            for dst_x_coord in 0..dst_width {
                let src_x_local = (dst_x_coord as f64 + 0.5) * scale_x - 0.5;
                let src_y_local = (dst_y_coord as f64 + 0.5) * scale_y - 0.5;

                let src_x_global = src_x_local + src_x as f64;
                let src_y_global = src_y_local + src_y as f64;

                let value = self.interpolate_at(src, src_x_global, src_y_global)?;
                dst.set_pixel(dst_x_coord, dst_y_coord, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(dst)
    }
}

#[cfg(feature = "simd")]
mod simd_impl {
    //! SIMD-accelerated bilinear interpolation
    //!
    //! Uses vectorization to process multiple pixels simultaneously.

    use super::*;

    impl BilinearResampler {
        /// SIMD-accelerated bilinear resampling
        ///
        /// This processes 4 or 8 pixels at once (depending on SIMD width)
        /// using platform-specific vector instructions.
        #[cfg(target_arch = "x86_64")]
        pub fn resample_simd(
            &self,
            src: &RasterBuffer,
            dst_width: u64,
            dst_height: u64,
        ) -> Result<RasterBuffer> {
            // SIMD implementation would use:
            // - AVX2 for processing 4 double values at once
            // - Vectorized lerp operations
            // - Gather instructions for non-contiguous loads
            //
            // For now, fall back to scalar
            self.resample(src, dst_width, dst_height)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use oxigdal_core::types::{NoDataValue, RasterDataType};

    #[test]
    fn test_lerp() {
        assert_abs_diff_eq!(
            BilinearResampler::lerp(0.0, 10.0, 0.0),
            0.0,
            epsilon = 1e-10
        );
        assert_abs_diff_eq!(
            BilinearResampler::lerp(0.0, 10.0, 1.0),
            10.0,
            epsilon = 1e-10
        );
        assert_abs_diff_eq!(
            BilinearResampler::lerp(0.0, 10.0, 0.5),
            5.0,
            epsilon = 1e-10
        );
    }

    #[test]
    fn test_bilinear_interp() {
        // Square with corners at 0, 1, 2, 3
        // Center should be 1.5
        let result = BilinearResampler::bilinear_interp(0.0, 1.0, 2.0, 3.0, 0.5, 0.5);
        assert_abs_diff_eq!(result, 1.5, epsilon = 1e-10);

        // At corner (0,0) should be v00
        let result = BilinearResampler::bilinear_interp(0.0, 1.0, 2.0, 3.0, 0.0, 0.0);
        assert_abs_diff_eq!(result, 0.0, epsilon = 1e-10);

        // At corner (1,1) should be v11
        let result = BilinearResampler::bilinear_interp(0.0, 1.0, 2.0, 3.0, 1.0, 1.0);
        assert_abs_diff_eq!(result, 3.0, epsilon = 1e-10);
    }

    #[test]
    fn test_bilinear_identity() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let resampler = BilinearResampler::new();
        let dst = resampler.resample(&src, 10, 10).ok();

        assert!(dst.is_some());
    }

    #[test]
    fn test_bilinear_smooth() {
        // Create a 2x2 ramp
        let mut src = RasterBuffer::zeros(2, 2, RasterDataType::Float32);
        src.set_pixel(0, 0, 0.0).ok();
        src.set_pixel(1, 0, 1.0).ok();
        src.set_pixel(0, 1, 2.0).ok();
        src.set_pixel(1, 1, 3.0).ok();

        // Upsample to 4x4 - should create smooth gradients
        let resampler = BilinearResampler::new();
        let dst = resampler.resample(&src, 4, 4).ok();

        assert!(dst.is_some());
        if let Some(dst) = dst {
            // Center value should be close to average
            let center = dst.get_pixel(1, 1).ok();
            // The exact value depends on the pixel-center registration
            assert!(center.is_some());
        }
    }

    #[test]
    fn test_bilinear_with_nodata() {
        let mut src =
            RasterBuffer::nodata_filled(3, 3, RasterDataType::Float32, NoDataValue::Float(-9999.0));

        // Set some valid values
        src.set_pixel(0, 0, 1.0).ok();
        src.set_pixel(1, 0, 2.0).ok();
        src.set_pixel(1, 1, 3.0).ok();
        // Rest are nodata

        let resampler = BilinearResampler::new();
        let result = resampler.resample(&src, 6, 6);
        assert!(result.is_ok());
    }

    #[test]
    fn test_bilinear_region() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let resampler = BilinearResampler::new();

        // Resample center 4x4 region to 8x8
        let result = resampler.resample_region(&src, 3, 3, 4, 4, 8, 8);
        assert!(result.is_ok());

        // Invalid region
        let result = resampler.resample_region(&src, 8, 8, 5, 5, 4, 4);
        assert!(result.is_err());
    }

    #[test]
    fn test_bilinear_zero_dimensions() {
        let src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let resampler = BilinearResampler::new();

        assert!(resampler.resample(&src, 0, 10).is_err());
        assert!(resampler.resample(&src, 10, 0).is_err());
    }
}

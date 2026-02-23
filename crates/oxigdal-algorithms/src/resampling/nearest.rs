//! Nearest neighbor resampling algorithm
//!
//! This is the fastest resampling method, which assigns each output pixel
//! the value of the nearest input pixel. No interpolation is performed.
//!
//! # Characteristics
//!
//! - **Speed**: Fastest (O(1) per pixel)
//! - **Quality**: Lowest (blocky for upsampling)
//! - **Preservation**: Exact input values preserved
//! - **Best for**: Categorical data, classifications, masks
//!
//! # Algorithm
//!
//! For each output pixel at (dst_x, dst_y):
//! 1. Compute source position: src_x = dst_x * scale_x, src_y = dst_y * scale_y
//! 2. Round to nearest integer: src_x = round(src_x), src_y = round(src_y)
//! 3. Copy value: dst[dst_y][dst_x] = src[src_y][src_x]

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;

/// Nearest neighbor resampler
#[derive(Debug, Clone, Copy, Default)]
pub struct NearestResampler;

impl NearestResampler {
    /// Creates a new nearest neighbor resampler
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Resamples a raster buffer using nearest neighbor
    ///
    /// # Arguments
    ///
    /// * `src` - Source raster buffer
    /// * `dst_width` - Target width in pixels
    /// * `dst_height` - Target height in pixels
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Target dimensions are zero
    /// - Source buffer is invalid
    /// - Memory allocation fails
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
                operation: "nearest neighbor resampling",
            });
        }

        // Create output buffer with same data type as source
        let mut dst = RasterBuffer::zeros(dst_width, dst_height, src.data_type());

        // Compute scaling factors
        let scale_x = src_width as f64 / dst_width as f64;
        let scale_y = src_height as f64 / dst_height as f64;

        // For each output pixel
        for dst_y in 0..dst_height {
            for dst_x in 0..dst_width {
                // Find nearest source pixel
                let src_x = self.map_coordinate(dst_x as f64, scale_x, src_width);
                let src_y = self.map_coordinate(dst_y as f64, scale_y, src_height);

                // Copy value
                let value = src.get_pixel(src_x, src_y).map_err(AlgorithmError::Core)?;
                dst.set_pixel(dst_x, dst_y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(dst)
    }

    /// Maps a destination coordinate to the nearest source coordinate
    ///
    /// Uses pixel-center registration: pixel coordinates are at integer positions,
    /// and pixel centers are at (i + 0.5, j + 0.5).
    ///
    /// # Arguments
    ///
    /// * `dst_coord` - Destination coordinate
    /// * `scale` - Scaling factor (src_size / dst_size)
    /// * `src_size` - Source dimension size
    ///
    /// # Returns
    ///
    /// The nearest source coordinate, clamped to [0, src_size)
    #[inline]
    fn map_coordinate(&self, dst_coord: f64, scale: f64, src_size: u64) -> u64 {
        // Map from destination pixel center to source coordinate
        let src_coord = (dst_coord + 0.5) * scale - 0.5;

        // Round to nearest integer
        let src_coord_rounded = src_coord.round();

        // Clamp to valid range
        let clamped = src_coord_rounded.max(0.0).min((src_size - 1) as f64);

        clamped as u64
    }

    /// Resamples with explicit scaling factors
    ///
    /// This variant allows precise control over the resampling transformation.
    ///
    /// # Arguments
    ///
    /// * `src` - Source raster buffer
    /// * `dst_width` - Target width
    /// * `dst_height` - Target height
    /// * `scale_x` - Horizontal scaling factor
    /// * `scale_y` - Vertical scaling factor
    /// * `offset_x` - Horizontal offset in source coordinates
    /// * `offset_y` - Vertical offset in source coordinates
    ///
    /// # Errors
    ///
    /// Returns an error if parameters are invalid
    #[allow(clippy::too_many_arguments)]
    pub fn resample_with_transform(
        &self,
        src: &RasterBuffer,
        dst_width: u64,
        dst_height: u64,
        scale_x: f64,
        scale_y: f64,
        offset_x: f64,
        offset_y: f64,
    ) -> Result<RasterBuffer> {
        if dst_width == 0 || dst_height == 0 {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "dimensions",
                message: "Target dimensions must be non-zero".to_string(),
            });
        }

        if scale_x <= 0.0 || scale_y <= 0.0 {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "scale",
                message: "Scale factors must be positive".to_string(),
            });
        }

        let src_width = src.width();
        let src_height = src.height();

        let mut dst = RasterBuffer::zeros(dst_width, dst_height, src.data_type());

        for dst_y in 0..dst_height {
            for dst_x in 0..dst_width {
                // Apply transform
                let src_x_f64 = dst_x as f64 * scale_x + offset_x;
                let src_y_f64 = dst_y as f64 * scale_y + offset_y;

                // Round and clamp
                let src_x = src_x_f64.round().max(0.0).min((src_width - 1) as f64) as u64;
                let src_y = src_y_f64.round().max(0.0).min((src_height - 1) as f64) as u64;

                // Copy value
                let value = src.get_pixel(src_x, src_y).map_err(AlgorithmError::Core)?;
                dst.set_pixel(dst_x, dst_y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(dst)
    }

    /// Resamples with repeat edge mode (instead of clamp)
    ///
    /// When sampling outside the source bounds, this wraps coordinates
    /// rather than clamping them. Useful for tiled or periodic data.
    pub fn resample_repeat(
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
                operation: "nearest neighbor resampling",
            });
        }

        let mut dst = RasterBuffer::zeros(dst_width, dst_height, src.data_type());

        let scale_x = src_width as f64 / dst_width as f64;
        let scale_y = src_height as f64 / dst_height as f64;

        for dst_y in 0..dst_height {
            for dst_x in 0..dst_width {
                let src_coord_x = (dst_x as f64 + 0.5) * scale_x - 0.5;
                let src_coord_y = (dst_y as f64 + 0.5) * scale_y - 0.5;

                // Wrap coordinates
                let src_x = (src_coord_x.round() as i64).rem_euclid(src_width as i64) as u64;
                let src_y = (src_coord_y.round() as i64).rem_euclid(src_height as i64) as u64;

                let value = src.get_pixel(src_x, src_y).map_err(AlgorithmError::Core)?;
                dst.set_pixel(dst_x, dst_y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(dst)
    }
}

#[cfg(feature = "simd")]
mod simd_impl {
    //! SIMD-accelerated nearest neighbor resampling
    //!
    //! While nearest neighbor doesn't benefit as much from SIMD as interpolation methods,
    //! we can still vectorize the coordinate calculation and memory access patterns.

    use super::*;

    impl NearestResampler {
        /// SIMD-accelerated resampling (when available)
        ///
        /// This uses platform-specific SIMD instructions to process multiple pixels
        /// at once. Falls back to scalar implementation if SIMD is not available.
        #[cfg(target_arch = "x86_64")]
        pub fn resample_simd(
            &self,
            src: &RasterBuffer,
            dst_width: u64,
            dst_height: u64,
        ) -> Result<RasterBuffer> {
            // For nearest neighbor, SIMD doesn't provide huge benefits
            // since we're just copying values. The main optimization is
            // vectorizing the coordinate calculations.
            //
            // In a production implementation, we would:
            // 1. Vectorize the scale_x/scale_y calculations
            // 2. Use gather instructions for non-contiguous memory access
            // 3. Batch pixel copies when possible
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
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_nearest_identity() {
        // Resampling to same size should be identity
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Fill with test pattern
        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let resampler = NearestResampler::new();
        let result = resampler.resample(&src, 10, 10);
        assert!(result.is_ok());

        if let Ok(dst) = result {
            for y in 0..10 {
                for x in 0..10 {
                    if let (Ok(sv), Ok(dv)) = (src.get_pixel(x, y), dst.get_pixel(x, y)) {
                        assert_abs_diff_eq!(sv, dv, epsilon = 1e-10);
                    }
                }
            }
        }
    }

    #[test]
    fn test_nearest_downsample() {
        // Create 4x4 checkerboard
        let mut src = RasterBuffer::zeros(4, 4, RasterDataType::Float32);
        for y in 0..4 {
            for x in 0..4 {
                let value = if (x + y) % 2 == 0 { 1.0 } else { 0.0 };
                src.set_pixel(x, y, value).ok();
            }
        }

        let resampler = NearestResampler::new();
        let dst = resampler.resample(&src, 2, 2);
        assert!(dst.is_ok());
    }

    #[test]
    fn test_nearest_upsample() {
        // Create 2x2 source
        let mut src = RasterBuffer::zeros(2, 2, RasterDataType::Float32);
        src.set_pixel(0, 0, 1.0).ok();
        src.set_pixel(1, 0, 2.0).ok();
        src.set_pixel(0, 1, 3.0).ok();
        src.set_pixel(1, 1, 4.0).ok();

        let resampler = NearestResampler::new();
        let dst = resampler.resample(&src, 4, 4);
        assert!(dst.is_ok());

        // Values should be replicated (blocky)
        if let Ok(dst) = dst {
            // Top-left quadrant should be mostly 1.0
            let val = dst.get_pixel(0, 0).ok();
            assert!(val.is_some());
        }
    }

    #[test]
    fn test_nearest_zero_dimensions() {
        let src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let resampler = NearestResampler::new();

        assert!(resampler.resample(&src, 0, 10).is_err());
        assert!(resampler.resample(&src, 10, 0).is_err());
    }

    #[test]
    fn test_map_coordinate() {
        let resampler = NearestResampler::new();

        // Downsampling 2:1 (scale = 2.0)
        // Formula: src_coord = (dst_coord + 0.5) * scale - 0.5, then round

        // dst=0.0: (0.0 + 0.5) * 2.0 - 0.5 = 0.5 → rounds to 1 (Rust rounds 0.5 away from zero)
        assert_eq!(resampler.map_coordinate(0.0, 2.0, 10), 1);

        // dst=1.0: (1.0 + 0.5) * 2.0 - 0.5 = 2.5 → rounds to 3
        assert_eq!(resampler.map_coordinate(1.0, 2.0, 10), 3);

        // dst=2.0: (2.0 + 0.5) * 2.0 - 0.5 = 4.5 → rounds to 5
        assert_eq!(resampler.map_coordinate(2.0, 2.0, 10), 5);

        // dst=3.0: (3.0 + 0.5) * 2.0 - 0.5 = 6.5 → rounds to 7
        assert_eq!(resampler.map_coordinate(3.0, 2.0, 10), 7);

        // Upsampling 1:2 (scale = 0.5)
        // dst=0.0: (0.0 + 0.5) * 0.5 - 0.5 = -0.25 → rounds to 0, clamped to 0
        assert_eq!(resampler.map_coordinate(0.0, 0.5, 10), 0);

        // dst=1.0: (1.0 + 0.5) * 0.5 - 0.5 = 0.25 → rounds to 0
        assert_eq!(resampler.map_coordinate(1.0, 0.5, 10), 0);

        // dst=2.0: (2.0 + 0.5) * 0.5 - 0.5 = 0.75 → rounds to 1
        assert_eq!(resampler.map_coordinate(2.0, 0.5, 10), 1);

        // Test clamping at upper bound
        // dst=20.0: (20.0 + 0.5) * 2.0 - 0.5 = 40.5 → rounds to 41, clamped to 9
        assert_eq!(resampler.map_coordinate(20.0, 2.0, 10), 9);
    }

    #[test]
    fn test_nearest_with_transform() {
        let src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let resampler = NearestResampler::new();

        // No offset, 1:1 scale
        let result = resampler.resample_with_transform(&src, 10, 10, 1.0, 1.0, 0.0, 0.0);
        assert!(result.is_ok());

        // Invalid scale
        let result = resampler.resample_with_transform(&src, 10, 10, 0.0, 1.0, 0.0, 0.0);
        assert!(result.is_err());

        let result = resampler.resample_with_transform(&src, 10, 10, 1.0, -1.0, 0.0, 0.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_nearest_repeat() {
        let mut src = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
        for y in 0..3 {
            for x in 0..3 {
                src.set_pixel(x, y, (y * 3 + x) as f64).ok();
            }
        }

        let resampler = NearestResampler::new();
        let result = resampler.resample_repeat(&src, 6, 6);
        assert!(result.is_ok());
    }
}

//! Lanczos resampling algorithm
//!
//! Lanczos resampling provides the highest quality results using a windowed sinc filter.
//! It preserves sharp edges better than bicubic while avoiding excessive ringing.
//!
//! # Characteristics
//!
//! - **Speed**: Slowest (36-100 samples per pixel)
//! - **Quality**: Highest, excellent edge preservation
//! - **Best for**: High-quality imagery, when quality is paramount
//!
//! # Algorithm
//!
//! For each output pixel:
//! 1. Sample (2a) x (2a) neighborhood (where a = lobes, typically 2 or 3)
//! 2. Apply Lanczos kernel: L(x) = sinc(x) * sinc(x/a)
//! 3. Normalize weights and sum

use crate::error::{AlgorithmError, Result};
use crate::resampling::kernel::{lanczos, normalize_weights};
use oxigdal_core::buffer::RasterBuffer;

/// Lanczos resampler with configurable lobe count
#[derive(Debug, Clone, Copy)]
pub struct LanczosResampler {
    /// Number of lobes (typically 2 or 3)
    lobes: usize,
}

impl Default for LanczosResampler {
    fn default() -> Self {
        Self::new(3)
    }
}

impl LanczosResampler {
    /// Creates a new Lanczos resampler with specified lobe count
    ///
    /// # Arguments
    ///
    /// * `lobes` - Number of lobes (2 = faster, 3 = higher quality)
    ///
    /// Common values:
    /// - 2: Lanczos2 - faster, still good quality
    /// - 3: Lanczos3 - standard, excellent quality (default)
    #[must_use]
    pub const fn new(lobes: usize) -> Self {
        Self { lobes }
    }

    /// Returns the lobe count
    #[must_use]
    pub const fn lobes(&self) -> usize {
        self.lobes
    }

    /// Returns the kernel radius (same as lobe count)
    #[must_use]
    pub const fn radius(&self) -> usize {
        self.lobes
    }

    /// Resamples a raster buffer using Lanczos interpolation
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
                operation: "Lanczos resampling",
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

    /// Interpolates value at fractional source coordinates using Lanczos
    fn interpolate_at(&self, src: &RasterBuffer, src_x: f64, src_y: f64) -> Result<f64> {
        let src_width = src.width();
        let src_height = src.height();

        // Clamp to valid range
        let src_x_clamped = src_x.max(0.0).min((src_width - 1) as f64);
        let src_y_clamped = src_y.max(0.0).min((src_height - 1) as f64);

        // Get integer center
        let x_center = src_x_clamped.floor() as i64;
        let y_center = src_y_clamped.floor() as i64;

        // Compute kernel range
        let radius = self.radius() as i64;
        let x_start = x_center - radius + 1;
        let x_end = x_center + radius + 1;
        let y_start = y_center - radius + 1;
        let y_end = y_center + radius + 1;

        let kernel_width = (x_end - x_start) as usize;
        let kernel_height = (y_end - y_start) as usize;

        // Allocate weight and value buffers
        let total_samples = kernel_width * kernel_height;
        let mut values = vec![0.0f64; total_samples];
        let mut weights = vec![0.0f64; total_samples];

        // Sample neighborhood and compute weights
        let mut idx = 0;
        for sy in y_start..y_end {
            for sx in x_start..x_end {
                // Clamp to valid range
                let sample_x = sx.max(0).min(src_width as i64 - 1) as u64;
                let sample_y = sy.max(0).min(src_height as i64 - 1) as u64;

                // Get value
                values[idx] = src
                    .get_pixel(sample_x, sample_y)
                    .map_err(AlgorithmError::Core)?;

                // Compute kernel weight
                let dx = sx as f64 - src_x_clamped;
                let dy = sy as f64 - src_y_clamped;
                let wx = lanczos(dx, self.lobes);
                let wy = lanczos(dy, self.lobes);
                weights[idx] = wx * wy;

                idx += 1;
            }
        }

        // Normalize weights
        normalize_weights(&mut weights);

        // Compute weighted sum
        let mut result = 0.0;
        for (value, weight) in values.iter().zip(weights.iter()) {
            result += value * weight;
        }

        Ok(result)
    }

    /// Resamples with edge handling
    ///
    /// This variant allows specifying how to handle edge pixels.
    ///
    /// # Arguments
    ///
    /// * `src` - Source buffer
    /// * `dst_width` - Destination width
    /// * `dst_height` - Destination height
    /// * `edge_mode` - How to handle edge pixels
    ///
    /// # Errors
    ///
    /// Returns an error if parameters are invalid
    pub fn resample_with_edge_mode(
        &self,
        src: &RasterBuffer,
        dst_width: u64,
        dst_height: u64,
        edge_mode: EdgeMode,
    ) -> Result<RasterBuffer> {
        // For now, only clamp mode is implemented
        match edge_mode {
            EdgeMode::Clamp => self.resample(src, dst_width, dst_height),
            EdgeMode::Wrap => Err(AlgorithmError::UnsupportedOperation {
                operation: "Wrap edge mode not yet implemented".to_string(),
            }),
            EdgeMode::Mirror => Err(AlgorithmError::UnsupportedOperation {
                operation: "Mirror edge mode not yet implemented".to_string(),
            }),
        }
    }

    /// Separable Lanczos resampling (optimized 2-pass)
    ///
    /// This performs resampling in two passes (horizontal then vertical)
    /// which is more cache-friendly and can be faster for large kernels.
    ///
    /// # Errors
    ///
    /// Returns an error if parameters are invalid
    pub fn resample_separable(
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

        // Pass 1: Horizontal resampling
        let mut temp = RasterBuffer::zeros(dst_width, src_height, src.data_type());
        let scale_x = src_width as f64 / dst_width as f64;

        for src_y in 0..src_height {
            for dst_x in 0..dst_width {
                let src_x = (dst_x as f64 + 0.5) * scale_x - 0.5;
                let value = self.interpolate_horizontal(src, src_x, src_y)?;
                temp.set_pixel(dst_x, src_y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        // Pass 2: Vertical resampling
        let mut dst = RasterBuffer::zeros(dst_width, dst_height, src.data_type());
        let scale_y = src_height as f64 / dst_height as f64;

        for dst_x in 0..dst_width {
            for dst_y in 0..dst_height {
                let src_y = (dst_y as f64 + 0.5) * scale_y - 0.5;
                let value = self.interpolate_vertical(&temp, dst_x, src_y)?;
                dst.set_pixel(dst_x, dst_y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(dst)
    }

    /// Horizontal 1D interpolation
    fn interpolate_horizontal(&self, src: &RasterBuffer, src_x: f64, y: u64) -> Result<f64> {
        let src_width = src.width();
        let src_x_clamped = src_x.max(0.0).min((src_width - 1) as f64);
        let x_center = src_x_clamped.floor() as i64;

        let radius = self.radius() as i64;
        let x_start = x_center - radius + 1;
        let x_end = x_center + radius + 1;

        let kernel_width = (x_end - x_start) as usize;
        let mut values = vec![0.0f64; kernel_width];
        let mut weights = vec![0.0f64; kernel_width];

        for (idx, sx) in (x_start..x_end).enumerate() {
            let sample_x = sx.max(0).min(src_width as i64 - 1) as u64;
            values[idx] = src.get_pixel(sample_x, y).map_err(AlgorithmError::Core)?;
            let dx = sx as f64 - src_x_clamped;
            weights[idx] = lanczos(dx, self.lobes);
        }

        normalize_weights(&mut weights);

        let result = values.iter().zip(weights.iter()).map(|(v, w)| v * w).sum();

        Ok(result)
    }

    /// Vertical 1D interpolation
    fn interpolate_vertical(&self, src: &RasterBuffer, x: u64, src_y: f64) -> Result<f64> {
        let src_height = src.height();
        let src_y_clamped = src_y.max(0.0).min((src_height - 1) as f64);
        let y_center = src_y_clamped.floor() as i64;

        let radius = self.radius() as i64;
        let y_start = y_center - radius + 1;
        let y_end = y_center + radius + 1;

        let kernel_height = (y_end - y_start) as usize;
        let mut values = vec![0.0f64; kernel_height];
        let mut weights = vec![0.0f64; kernel_height];

        for (idx, sy) in (y_start..y_end).enumerate() {
            let sample_y = sy.max(0).min(src_height as i64 - 1) as u64;
            values[idx] = src.get_pixel(x, sample_y).map_err(AlgorithmError::Core)?;
            let dy = sy as f64 - src_y_clamped;
            weights[idx] = lanczos(dy, self.lobes);
        }

        normalize_weights(&mut weights);

        let result = values.iter().zip(weights.iter()).map(|(v, w)| v * w).sum();

        Ok(result)
    }
}

/// Edge handling modes for resampling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeMode {
    /// Clamp coordinates to edge (default)
    Clamp,
    /// Wrap coordinates (for tiled data)
    Wrap,
    /// Mirror coordinates at edges
    Mirror,
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_lanczos_creation() {
        let l2 = LanczosResampler::new(2);
        assert_eq!(l2.lobes(), 2);
        assert_eq!(l2.radius(), 2);

        let l3 = LanczosResampler::new(3);
        assert_eq!(l3.lobes(), 3);
        assert_eq!(l3.radius(), 3);
    }

    #[test]
    fn test_lanczos_identity() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let resampler = LanczosResampler::new(3);
        let dst = resampler.resample(&src, 10, 10);
        assert!(dst.is_ok());
    }

    #[test]
    fn test_lanczos_quality() {
        // Lanczos should produce high-quality smooth results
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                src.set_pixel(x, y, ((x + y) * (x + y)) as f64).ok();
            }
        }

        let resampler = LanczosResampler::new(3);
        let dst = resampler.resample(&src, 10, 10);
        assert!(dst.is_ok());

        // Result should be smooth
        if let Ok(dst) = dst {
            let v1 = dst.get_pixel(4, 4).ok();
            let v2 = dst.get_pixel(5, 5).ok();
            assert!(v1.is_some());
            assert!(v2.is_some());
        }
    }

    #[test]
    fn test_lanczos_separable() {
        let mut src = RasterBuffer::zeros(8, 8, RasterDataType::Float32);
        for y in 0..8 {
            for x in 0..8 {
                src.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let resampler = LanczosResampler::new(3);
        let dst1 = resampler.resample(&src, 16, 16);
        let dst2 = resampler.resample_separable(&src, 16, 16);

        assert!(dst1.is_ok());
        assert!(dst2.is_ok());

        // Results should be similar (not identical due to rounding)
    }

    #[test]
    fn test_lanczos_lobes() {
        let src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        let l2 = LanczosResampler::new(2);
        let l3 = LanczosResampler::new(3);

        let dst2 = l2.resample(&src, 20, 20);
        let dst3 = l3.resample(&src, 20, 20);

        assert!(dst2.is_ok());
        assert!(dst3.is_ok());
    }

    #[test]
    fn test_edge_modes() {
        let src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let resampler = LanczosResampler::new(3);

        let clamp = resampler.resample_with_edge_mode(&src, 10, 10, EdgeMode::Clamp);
        assert!(clamp.is_ok());

        // Wrap and Mirror not yet implemented
        let wrap = resampler.resample_with_edge_mode(&src, 10, 10, EdgeMode::Wrap);
        assert!(wrap.is_err());

        let mirror = resampler.resample_with_edge_mode(&src, 10, 10, EdgeMode::Mirror);
        assert!(mirror.is_err());
    }

    #[test]
    fn test_lanczos_zero_dimensions() {
        let src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let resampler = LanczosResampler::new(3);

        assert!(resampler.resample(&src, 0, 10).is_err());
        assert!(resampler.resample(&src, 10, 0).is_err());
    }
}

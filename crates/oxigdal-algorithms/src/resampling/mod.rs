//! Raster resampling algorithms
//!
//! This module provides various resampling algorithms for changing the resolution
//! or dimensions of raster data while maintaining spatial accuracy.
//!
//! # Algorithms
//!
//! - Nearest neighbor (fastest, preserves exact values)
//! - Bilinear interpolation (smooth, good for continuous data)
//! - Bicubic interpolation (higher quality, smoother)
//! - Lanczos resampling (highest quality, most expensive)
//!
//! # Example
//!
//! ```no_run
//! use oxigdal_algorithms::resampling::{ResamplingMethod, Resampler};
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//! # use oxigdal_algorithms::error::Result;
//!
//! # fn main() -> Result<()> {
//! // Create a resampler
//! let resampler = Resampler::new(ResamplingMethod::Bilinear);
//!
//! // Resample a raster buffer
//! let src = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//! let dst = resampler.resample(&src, 500, 500)?;
//! # Ok(())
//! # }
//! ```

mod bicubic;
mod bilinear;
mod kernel;
mod lanczos;
mod nearest;

pub use bicubic::BicubicResampler;
pub use bilinear::BilinearResampler;
pub use lanczos::LanczosResampler;
pub use nearest::NearestResampler;

use crate::error::Result;
use oxigdal_core::buffer::RasterBuffer;

/// Resampling methods
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ResamplingMethod {
    /// Nearest neighbor - fastest, preserves exact values
    ///
    /// Best for: Categorical data, classification maps
    /// Pros: Very fast, no new values introduced
    /// Cons: Blocky appearance when upsampling
    Nearest,

    /// Bilinear interpolation - smooth, moderate quality
    ///
    /// Best for: Continuous data, DEMs, temperature maps
    /// Pros: Smooth results, reasonably fast
    /// Cons: Some blurring, not ideal for downsampling
    #[default]
    Bilinear,

    /// Bicubic interpolation - high quality, smoother than bilinear
    ///
    /// Best for: High-quality imagery, DEMs requiring smoothness
    /// Pros: Very smooth, better edge preservation than bilinear
    /// Cons: Slower, can introduce slight ringing artifacts
    Bicubic,

    /// Lanczos resampling - highest quality, most expensive
    ///
    /// Best for: High-quality imagery, when quality matters most
    /// Pros: Excellent quality, sharp edges
    /// Cons: Slowest, can introduce ringing near sharp edges
    Lanczos,
}

impl ResamplingMethod {
    /// Returns a human-readable name for the method
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Nearest => "Nearest Neighbor",
            Self::Bilinear => "Bilinear",
            Self::Bicubic => "Bicubic",
            Self::Lanczos => "Lanczos",
        }
    }

    /// Returns the kernel radius for this method
    ///
    /// This is the number of pixels in each direction that are sampled
    /// for interpolation.
    #[must_use]
    pub const fn kernel_radius(&self) -> usize {
        match self {
            Self::Nearest => 0,
            Self::Bilinear => 1,
            Self::Bicubic => 2,
            Self::Lanczos => 3,
        }
    }
}

/// Generic resampler that can use any resampling method
pub struct Resampler {
    method: ResamplingMethod,
}

impl Resampler {
    /// Creates a new resampler with the specified method
    #[must_use]
    pub const fn new(method: ResamplingMethod) -> Self {
        Self { method }
    }

    /// Resamples a raster buffer to new dimensions
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
    /// - Data type is unsupported
    /// - Memory allocation fails
    pub fn resample(
        &self,
        src: &RasterBuffer,
        dst_width: u64,
        dst_height: u64,
    ) -> Result<RasterBuffer> {
        if dst_width == 0 || dst_height == 0 {
            return Err(crate::error::AlgorithmError::InvalidParameter {
                parameter: "dimensions",
                message: "Target dimensions must be non-zero".to_string(),
            });
        }

        match self.method {
            ResamplingMethod::Nearest => {
                let resampler = NearestResampler;
                resampler.resample(src, dst_width, dst_height)
            }
            ResamplingMethod::Bilinear => {
                let resampler = BilinearResampler;
                resampler.resample(src, dst_width, dst_height)
            }
            ResamplingMethod::Bicubic => {
                let resampler = BicubicResampler::new();
                resampler.resample(src, dst_width, dst_height)
            }
            ResamplingMethod::Lanczos => {
                let resampler = LanczosResampler::new(3);
                resampler.resample(src, dst_width, dst_height)
            }
        }
    }

    /// Returns the resampling method
    #[must_use]
    pub const fn method(&self) -> ResamplingMethod {
        self.method
    }
}

impl Default for Resampler {
    fn default() -> Self {
        Self::new(ResamplingMethod::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_resampling_method_names() {
        assert_eq!(ResamplingMethod::Nearest.name(), "Nearest Neighbor");
        assert_eq!(ResamplingMethod::Bilinear.name(), "Bilinear");
        assert_eq!(ResamplingMethod::Bicubic.name(), "Bicubic");
        assert_eq!(ResamplingMethod::Lanczos.name(), "Lanczos");
    }

    #[test]
    fn test_kernel_radius() {
        assert_eq!(ResamplingMethod::Nearest.kernel_radius(), 0);
        assert_eq!(ResamplingMethod::Bilinear.kernel_radius(), 1);
        assert_eq!(ResamplingMethod::Bicubic.kernel_radius(), 2);
        assert_eq!(ResamplingMethod::Lanczos.kernel_radius(), 3);
    }

    #[test]
    fn test_resampler_creation() {
        let resampler = Resampler::new(ResamplingMethod::Bilinear);
        assert_eq!(resampler.method(), ResamplingMethod::Bilinear);
    }

    #[test]
    fn test_resample_zero_dimensions() {
        let src = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
        let resampler = Resampler::new(ResamplingMethod::Nearest);

        assert!(resampler.resample(&src, 0, 100).is_err());
        assert!(resampler.resample(&src, 100, 0).is_err());
    }
}

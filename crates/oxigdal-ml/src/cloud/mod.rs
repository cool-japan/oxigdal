//! Cloud detection and removal for satellite imagery
//!
//! This module provides cloud detection and removal capabilities for
//! multi-spectral satellite imagery (Sentinel-2, Landsat, etc.).
//!
//! # Features
//!
//! - **Cloud Detection**: ONNX-based cloud mask generation with confidence levels
//! - **Cloud Removal**: Inpainting using partial convolutions and temporal interpolation
//! - **Multi-Spectral Support**: Sentinel-2 and Landsat imagery
//! - **Morphological Operations**: Mask refinement (dilation, erosion)
//!
//! # Implementation Status
//!
//! **Note**: Full ONNX Runtime 2.0 integration is pending. Currently uses rule-based
//! fallback detection. The interface is stable and will support ONNX models when
//! ort 2.0 integration is complete.
//!
//! # Example: Cloud Detection
//!
//! ```ignore
//! # #[cfg(feature = "cloud-removal")]
//! # {
//! use oxigdal_ml::cloud::{CloudDetector, CloudConfig};
//! use oxigdal_core::buffer::RasterBuffer;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create detector with ONNX model
//! let config = CloudConfig::sentinel2();
//! let detector = CloudDetector::from_file("cloud_model.onnx", config)?;
//!
//! // Detect clouds in multi-spectral image
//! let image: RasterBuffer = /* load image */;
//! # RasterBuffer::zeros(512, 512, oxigdal_core::types::RasterDataType::Float32);
//! let mask = detector.detect(&image)?;
//! # Ok(())
//! # }
//! # }
//! ```
//!
//! # Example: Cloud Removal
//!
//! ```ignore
//! # #[cfg(feature = "cloud-removal")]
//! # {
//! use oxigdal_ml::cloud::{CloudRemover, CloudConfig};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let config = CloudConfig::default();
//! let remover = CloudRemover::new(config);
//!
//! // Remove clouds from image with mask
//! let cleaned = remover.remove(&image, &mask)?;
//! # Ok(())
//! # }
//! # }
//! ```

mod detection;
mod removal;

pub use detection::{CloudDetector, CloudMask};
pub use removal::CloudRemover;

use serde::{Deserialize, Serialize};

/// Cloud detection and removal configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudConfig {
    /// Cloud confidence threshold (0.0 to 1.0)
    pub confidence_threshold: f32,
    /// Mask dilation radius (pixels) for shadow detection
    pub dilation_radius: usize,
    /// Mask erosion radius (pixels) for refinement
    pub erosion_radius: usize,
    /// Band indices for cloud detection
    pub band_indices: Vec<usize>,
    /// Band normalization mean values
    pub normalization_mean: Vec<f32>,
    /// Band normalization std values
    pub normalization_std: Vec<f32>,
    /// Use temporal interpolation if available
    pub use_temporal: bool,
    /// Blending alpha for inpainting
    pub blend_alpha: f32,
}

impl CloudConfig {
    /// Sentinel-2 optimized configuration
    #[must_use]
    pub fn sentinel2() -> Self {
        Self {
            confidence_threshold: 0.5,
            dilation_radius: 5,
            erosion_radius: 2,
            // B2 (Blue), B3 (Green), B4 (Red), B8 (NIR), B11 (SWIR1), B12 (SWIR2)
            band_indices: vec![1, 2, 3, 7, 10, 11],
            normalization_mean: vec![0.1340, 0.1447, 0.1374, 0.2982, 0.2035, 0.1416],
            normalization_std: vec![0.0356, 0.0390, 0.0484, 0.0651, 0.0717, 0.0746],
            use_temporal: false,
            blend_alpha: 0.8,
        }
    }

    /// Landsat 8/9 optimized configuration
    #[must_use]
    pub fn landsat() -> Self {
        Self {
            confidence_threshold: 0.5,
            dilation_radius: 5,
            erosion_radius: 2,
            // B2 (Blue), B3 (Green), B4 (Red), B5 (NIR), B6 (SWIR1), B7 (SWIR2)
            band_indices: vec![1, 2, 3, 4, 5, 6],
            normalization_mean: vec![0.1320, 0.1450, 0.1380, 0.2950, 0.2000, 0.1400],
            normalization_std: vec![0.0350, 0.0400, 0.0480, 0.0650, 0.0720, 0.0750],
            use_temporal: false,
            blend_alpha: 0.8,
        }
    }
}

impl Default for CloudConfig {
    fn default() -> Self {
        Self::sentinel2()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sentinel2_config() {
        let config = CloudConfig::sentinel2();
        assert_eq!(config.band_indices.len(), 6);
        assert_eq!(config.confidence_threshold, 0.5);
    }

    #[test]
    fn test_landsat_config() {
        let config = CloudConfig::landsat();
        assert_eq!(config.band_indices.len(), 6);
        assert!(config.dilation_radius > 0);
    }
}

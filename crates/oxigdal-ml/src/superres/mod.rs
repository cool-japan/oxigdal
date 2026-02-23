//! Super-resolution for geospatial imagery
//!
//! This module provides super-resolution capabilities using ONNX models
//! (Real-ESRGAN and similar architectures) with tile-based processing
//! to handle large rasters efficiently.
//!
//! # Features
//!
//! - Tile-based processing with overlap blending
//! - Support for 2x and 4x upsampling
//! - Memory-efficient batch processing
//! - Preserves geospatial metadata (GeoTransform)
//! - Edge case handling for tiles at boundaries
//!
//! # Example
//!
//! ```no_run
//! use oxigdal_ml::superres::{SuperResolution, SuperResConfig};
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create configuration
//! let config = SuperResConfig::new(2, 256, 32);
//!
//! // Load model
//! let mut model = SuperResolution::from_file("real_esrgan_2x.onnx", config)?;
//!
//! // Create input raster
//! let input = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
//!
//! // Upscale
//! let output = model.upscale(&input)?;
//! # Ok(())
//! # }
//! ```

mod model;

pub use model::{SuperResConfig, SuperResolution};

use crate::error::Result;
use oxigdal_core::buffer::RasterBuffer;

/// Upscale factor for super-resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpscaleFactor {
    /// 2x upsampling
    X2 = 2,
    /// 4x upsampling
    X4 = 4,
}

impl UpscaleFactor {
    /// Convert to integer scale factor
    #[must_use]
    pub const fn as_usize(self) -> usize {
        self as usize
    }
}

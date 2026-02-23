//! Pan-sharpening algorithms
//!
//! Merge high-resolution panchromatic and lower-resolution multispectral images

pub mod brovey;
pub mod ihs;
pub mod pca;

pub use brovey::BroveyTransform;
pub use ihs::IHSPanSharpening;
pub use pca::PCAPanSharpening;

use crate::error::Result;
use scirs2_core::ndarray::Array2;

/// Pan-sharpening trait
pub trait PanSharpening {
    /// Sharpen multispectral band using panchromatic band
    fn sharpen(&self, ms: &Array2<f64>, pan: &Array2<f64>) -> Result<Array2<f64>>;
}

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
use scirs2_core::ndarray::{Array2, ArrayView2};

/// Pan-sharpening trait
pub trait PanSharpening {
    /// Sharpen multi-spectral bands using a panchromatic band.
    ///
    /// `ms_bands` — slice of 2-D arrays, one per spectral band (rows × cols).
    /// `pan`      — 2-D panchromatic array, same spatial size as each MS band.
    ///
    /// Returns one sharpened array per input MS band, in the same order.
    fn sharpen(
        &self,
        ms_bands: &[ArrayView2<f64>],
        pan: &ArrayView2<f64>,
    ) -> Result<Vec<Array2<f64>>>;
}

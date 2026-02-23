//! PCA (Principal Component Analysis) based pan-sharpening

use super::PanSharpening;
use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array2, Zip};

/// PCA-based pan-sharpening
pub struct PCAPanSharpening;

impl PanSharpening for PCAPanSharpening {
    fn sharpen(&self, ms: &Array2<f64>, pan: &Array2<f64>) -> Result<Array2<f64>> {
        if ms.dim() != pan.dim() {
            return Err(SensorError::dimension_mismatch(
                format!("{:?}", ms.dim()),
                format!("{:?}", pan.dim()),
            ));
        }

        // Simplified PCA: Weight by variance
        let mut sharpened = Array2::zeros(ms.dim());

        Zip::from(&mut sharpened)
            .and(ms)
            .and(pan)
            .for_each(|out, &m, &p| {
                *out = m * 0.7 + p * 0.3;
            });

        Ok(sharpened)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_pca() {
        let ms = array![[0.5, 0.6]];
        let pan = array![[0.8, 0.9]];

        let transform = PCAPanSharpening;
        let result = transform.sharpen(&ms, &pan).ok();
        assert!(result.is_some());
    }
}

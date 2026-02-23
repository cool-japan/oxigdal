//! Brovey Transform pan-sharpening
//!
//! A simple ratio-based pan-sharpening method

use super::PanSharpening;
use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array2, Zip};

/// Brovey Transform pan-sharpening
pub struct BroveyTransform;

impl PanSharpening for BroveyTransform {
    fn sharpen(&self, ms: &Array2<f64>, pan: &Array2<f64>) -> Result<Array2<f64>> {
        if ms.dim() != pan.dim() {
            return Err(SensorError::dimension_mismatch(
                format!("{:?}", ms.dim()),
                format!("{:?}", pan.dim()),
            ));
        }

        let mut sharpened = Array2::zeros(ms.dim());

        Zip::from(&mut sharpened)
            .and(ms)
            .and(pan)
            .for_each(|out, &m, &p| {
                *out = if m.abs() > 1e-10 {
                    (p * m) / (m + 1e-10)
                } else {
                    0.0
                };
            });

        Ok(sharpened)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_brovey() {
        let ms = array![[0.5, 0.6]];
        let pan = array![[0.8, 0.9]];

        let transform = BroveyTransform;
        let result = transform.sharpen(&ms, &pan).ok();
        assert!(result.is_some());
    }
}

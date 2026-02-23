//! IHS (Intensity-Hue-Saturation) pan-sharpening

use super::PanSharpening;
use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array2, Zip};

/// IHS pan-sharpening
pub struct IHSPanSharpening;

impl PanSharpening for IHSPanSharpening {
    fn sharpen(&self, ms: &Array2<f64>, pan: &Array2<f64>) -> Result<Array2<f64>> {
        if ms.dim() != pan.dim() {
            return Err(SensorError::dimension_mismatch(
                format!("{:?}", ms.dim()),
                format!("{:?}", pan.dim()),
            ));
        }

        // Simplified IHS: Replace intensity with panchromatic
        let mut sharpened = Array2::zeros(ms.dim());

        Zip::from(&mut sharpened)
            .and(ms)
            .and(pan)
            .for_each(|out, &m, &p| {
                *out = m + (p - m) * 0.5;
            });

        Ok(sharpened)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_ihs() {
        let ms = array![[0.5, 0.6]];
        let pan = array![[0.8, 0.9]];

        let transform = IHSPanSharpening;
        let result = transform.sharpen(&ms, &pan).ok();
        assert!(result.is_some());
    }
}

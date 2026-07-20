//! Brovey Transform pan-sharpening
//!
//! Implements the ratio-based pan-sharpening method described in:
//! Pohl, C. & van Genderen, J.L. (1998) "Multisensor image fusion in remote sensing:
//! concepts, methods and applications." *International Journal of Remote Sensing* 19(5), 823–854.

use super::PanSharpening;
use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array2, ArrayView2};

/// Brovey Transform pan-sharpening.
///
/// For each pixel `(r, c)`:
/// - `I = Σ ms_bands[b][r,c]`  (total intensity over all input bands)
/// - `sharpened_b[r,c] = ms_bands[b][r,c] * pan[r,c] / max(I, ε)`
///
/// When all bands are zero the denominator guard `ε = 1e-10` is used, which
/// yields zero output (0 × pan / ε = 0) — no NaN or division-by-zero.
pub struct BroveyTransform;

impl PanSharpening for BroveyTransform {
    fn sharpen(
        &self,
        ms_bands: &[ArrayView2<f64>],
        pan: &ArrayView2<f64>,
    ) -> Result<Vec<Array2<f64>>> {
        if ms_bands.is_empty() {
            return Err(SensorError::pan_sharpening_error(
                "BroveyTransform requires at least one MS band",
            ));
        }

        let pan_shape = pan.dim();

        for (idx, band) in ms_bands.iter().enumerate() {
            if band.dim() != pan_shape {
                return Err(SensorError::dimension_mismatch(
                    format!("{:?}", pan_shape),
                    format!("band {idx}: {:?}", band.dim()),
                ));
            }
        }

        let (rows, cols) = pan_shape;
        let n_bands = ms_bands.len();

        let mut results: Vec<Array2<f64>> =
            (0..n_bands).map(|_| Array2::zeros((rows, cols))).collect();

        for r in 0..rows {
            for c in 0..cols {
                // Sum intensity across all bands for this pixel.
                let intensity: f64 = ms_bands.iter().map(|b| b[[r, c]]).sum();
                let denom = intensity.max(1e-10);
                let pan_val = pan[[r, c]];

                for (b, band) in ms_bands.iter().enumerate() {
                    results[b][[r, c]] = band[[r, c]] * pan_val / denom;
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::array;

    /// Basic three-band Brovey on a 2×2 image — compare against manual calculation.
    ///
    /// Bands: R=0.3, G=0.5, B=0.2  (constant across all pixels)
    /// Pan:   0.8
    /// I = 0.3 + 0.5 + 0.2 = 1.0
    /// R' = 0.3 * 0.8 / 1.0 = 0.24
    /// G' = 0.5 * 0.8 / 1.0 = 0.40
    /// B' = 0.2 * 0.8 / 1.0 = 0.16
    #[test]
    fn test_brovey_three_band_basic() {
        let r = array![[0.3_f64, 0.3], [0.3, 0.3]];
        let g = array![[0.5_f64, 0.5], [0.5, 0.5]];
        let b = array![[0.2_f64, 0.2], [0.2, 0.2]];
        let pan = array![[0.8_f64, 0.8], [0.8, 0.8]];

        let transform = BroveyTransform;
        let bands = [r.view(), g.view(), b.view()];
        let results = transform.sharpen(&bands, &pan.view()).unwrap();

        assert_eq!(results.len(), 3);
        for &v in results[0].iter() {
            let diff = (v - 0.24_f64).abs();
            assert!(diff < 1e-10, "R band mismatch: {v} vs 0.24");
        }
        for &v in results[1].iter() {
            let diff = (v - 0.40_f64).abs();
            assert!(diff < 1e-10, "G band mismatch: {v} vs 0.40");
        }
        for &v in results[2].iter() {
            let diff = (v - 0.16_f64).abs();
            assert!(diff < 1e-10, "B band mismatch: {v} vs 0.16");
        }
    }

    /// When all bands are zero at a pixel, the denominator guard must fire and
    /// the output must be 0.0 (not NaN or infinity).
    #[test]
    fn test_brovey_zero_intensity() {
        let z = array![[0.0_f64, 0.0]];
        let pan = array![[1.0_f64, 0.5]];

        let transform = BroveyTransform;
        let bands = [z.view(), z.view(), z.view()];
        let results = transform.sharpen(&bands, &pan.view()).unwrap();

        for result in &results {
            for &v in result.iter() {
                assert!(
                    v.is_finite() && v == 0.0,
                    "Expected 0.0 for zero-intensity pixel, got {v}"
                );
            }
        }
    }

    /// Output shape must equal pan shape.
    #[test]
    fn test_brovey_shape_invariant() {
        let ms = Array2::from_elem((4, 7), 0.5_f64);
        let pan = Array2::from_elem((4, 7), 0.9_f64);

        let transform = BroveyTransform;
        let bands = [ms.view(), ms.view()];
        let results = transform.sharpen(&bands, &pan.view()).unwrap();

        for result in &results {
            assert_eq!(result.dim(), (4, 7));
        }
    }
}

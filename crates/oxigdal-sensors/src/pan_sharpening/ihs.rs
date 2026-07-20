//! IHS (Intensity-Hue-Saturation) pan-sharpening
//!
//! Implements the IHS colour-space fusion method described in:
//! Carper, W., Lillesand, T. & Kiefer, R. (1990) "The use of Intensity-Hue-Saturation
//! transformations for merging SPOT panchromatic and multispectral image data."
//! *Photogrammetric Engineering and Remote Sensing* 56(4), 459–467.
//!
//! Algorithm (cylindrical-coordinate variant):
//! 1. Forward IHS: RGB → (I, H, S)
//! 2. Histogram-match Pan to I
//! 3. Inverse IHS: (Pan_matched, H, S) → R′ G′ B′

use super::PanSharpening;
use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array2, ArrayView2};

/// IHS pan-sharpening.  Requires exactly 3 MS bands ordered (R, G, B).
pub struct IHSPanSharpening;

// ── helpers ────────────────────────────────────────────────────────────────

/// Compute the IHS Intensity component `I = (R + G + B) / 3`.
#[inline]
fn ihs_intensity(r: f64, g: f64, b: f64) -> f64 {
    (r + g + b) / 3.0
}

/// Compute the IHS Hue component `H = atan2(√3·(G-B), 2R-G-B)`.
#[inline]
fn ihs_hue(r: f64, g: f64, b: f64) -> f64 {
    f64::atan2(3.0_f64.sqrt() * (g - b), 2.0 * r - g - b)
}

/// Compute the IHS Saturation component for the cylindrical-coordinate IHS
/// model used by the forward hue and the inverse transform in this module.
///
/// The saturation must be defined consistently with `ihs_hue` and the inverse
/// equations, otherwise the transform is not invertible. Writing the chroma
/// vector as `v1 = 2R-G-B` and `v2 = √3·(G-B)` (so `H = atan2(v2, v1)`), the
/// chroma magnitude is `M = √(v1² + v2²) = 6·I·S`. Hence
///
/// `S = √((2R-G-B)² + 3·(G-B)²) / (2·(R+G+B))` (`= M / (6·I)`).
///
/// This guarantees the round-trip property: substituting `I` with an identical
/// intensity recovers the original R, G, B exactly.
///
/// Guard: if `R+G+B < 1e-10` → `S = 0`.
#[inline]
fn ihs_saturation(r: f64, g: f64, b: f64) -> f64 {
    let sum = r + g + b;
    if sum < 1e-10 {
        return 0.0;
    }
    let v1 = 2.0 * r - g - b;
    let d = g - b;
    (v1 * v1 + 3.0 * d * d).sqrt() / (2.0 * sum)
}

/// Linear histogram-match: rescale `src_val` so that the distribution of
/// `src_val` spans the same range as the reference.
///
/// Returns the matched value for a single element.
#[inline]
fn histogram_match_value(
    src_val: f64,
    src_min: f64,
    src_range: f64,
    ref_min: f64,
    ref_range: f64,
) -> f64 {
    if src_range < 1e-12 {
        return ref_min;
    }
    (src_val - src_min) / src_range * ref_range + ref_min
}

// ── trait impl ─────────────────────────────────────────────────────────────

impl PanSharpening for IHSPanSharpening {
    fn sharpen(
        &self,
        ms_bands: &[ArrayView2<f64>],
        pan: &ArrayView2<f64>,
    ) -> Result<Vec<Array2<f64>>> {
        if ms_bands.len() != 3 {
            return Err(SensorError::pan_sharpening_error(
                "IHS requires exactly 3 bands (R, G, B)",
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

        // ── 1. Forward IHS transform ────────────────────────────────────────
        let mut intensity = Array2::<f64>::zeros((rows, cols));
        let mut hue = Array2::<f64>::zeros((rows, cols));
        let mut saturation = Array2::<f64>::zeros((rows, cols));

        for r in 0..rows {
            for c in 0..cols {
                let rv = ms_bands[0][[r, c]];
                let gv = ms_bands[1][[r, c]];
                let bv = ms_bands[2][[r, c]];
                intensity[[r, c]] = ihs_intensity(rv, gv, bv);
                hue[[r, c]] = ihs_hue(rv, gv, bv);
                saturation[[r, c]] = ihs_saturation(rv, gv, bv);
            }
        }

        // ── 2. Histogram-match Pan → I ──────────────────────────────────────
        let pan_min = pan.iter().copied().fold(f64::INFINITY, f64::min);
        let pan_max = pan.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let pan_range = pan_max - pan_min;

        let i_min = intensity.iter().copied().fold(f64::INFINITY, f64::min);
        let i_max = intensity.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let i_range = i_max - i_min;

        let mut pan_matched = Array2::<f64>::zeros((rows, cols));
        for r in 0..rows {
            for c in 0..cols {
                pan_matched[[r, c]] =
                    histogram_match_value(pan[[r, c]], pan_min, pan_range, i_min, i_range);
            }
        }

        // ── 3. Inverse IHS transform ────────────────────────────────────────
        let sqrt3 = 3.0_f64.sqrt();

        let mut r_out = Array2::<f64>::zeros((rows, cols));
        let mut g_out = Array2::<f64>::zeros((rows, cols));
        let mut b_out = Array2::<f64>::zeros((rows, cols));

        for r in 0..rows {
            for c in 0..cols {
                let i_new = pan_matched[[r, c]];
                let h = hue[[r, c]];
                let s = saturation[[r, c]];
                let cos_h = h.cos();
                let sin_h = h.sin();

                r_out[[r, c]] = i_new * (1.0 + 2.0 * s * cos_h);
                g_out[[r, c]] = i_new * (1.0 - s * (cos_h - sqrt3 * sin_h));
                b_out[[r, c]] = i_new * (1.0 - s * (cos_h + sqrt3 * sin_h));
            }
        }

        Ok(vec![r_out, g_out, b_out])
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::Array2;

    /// IHSPanSharpening must return an error when the input is not 3 bands.
    #[test]
    fn test_ihs_requires_three_bands() {
        let ms = Array2::from_elem((3, 3), 0.5_f64);
        let pan = Array2::from_elem((3, 3), 0.8_f64);
        let transform = IHSPanSharpening;

        // 2 bands — error
        let result = transform.sharpen(&[ms.view(), ms.view()], &pan.view());
        assert!(result.is_err(), "Expected error for 2-band input");

        // 4 bands — error
        let result = transform.sharpen(&[ms.view(), ms.view(), ms.view(), ms.view()], &pan.view());
        assert!(result.is_err(), "Expected error for 4-band input");
    }

    /// Inverse-roundtrip: when Pan == I (the intensity of the input RGB),
    /// the IHS substitution is a no-op and R′≈R, G′≈G, B′≈B.
    #[test]
    fn test_ihs_inverse_roundtrip_no_pan_change() {
        // Synthetic 4×4 input with varied per-pixel RGB values.
        let r_data: Vec<f64> = vec![
            0.2, 0.5, 0.8, 0.1, 0.4, 0.7, 0.3, 0.6, 0.9, 0.15, 0.45, 0.75, 0.25, 0.55, 0.85, 0.35,
        ];
        let g_data: Vec<f64> = vec![
            0.3, 0.4, 0.6, 0.2, 0.5, 0.8, 0.4, 0.7, 0.9, 0.25, 0.55, 0.65, 0.35, 0.45, 0.75, 0.25,
        ];
        let b_data: Vec<f64> = vec![
            0.1, 0.6, 0.7, 0.3, 0.6, 0.5, 0.2, 0.5, 0.8, 0.10, 0.35, 0.55, 0.15, 0.65, 0.65, 0.45,
        ];

        use scirs2_core::ndarray::Array;
        let r_band = Array::from_shape_vec((4, 4), r_data.clone()).unwrap();
        let g_band = Array::from_shape_vec((4, 4), g_data.clone()).unwrap();
        let b_band = Array::from_shape_vec((4, 4), b_data.clone()).unwrap();

        // Build Pan = I (the intensity of the input bands).
        let mut pan_data = vec![0.0_f64; 16];
        for i in 0..16 {
            pan_data[i] = (r_data[i] + g_data[i] + b_data[i]) / 3.0;
        }
        let pan_band = Array::from_shape_vec((4, 4), pan_data).unwrap();

        let transform = IHSPanSharpening;
        let results = transform
            .sharpen(
                &[r_band.view(), g_band.view(), b_band.view()],
                &pan_band.view(),
            )
            .unwrap();

        // With Pan == I the histogram matching is identity (range identical),
        // so the inverse transform should recover the original bands.
        for (r, c) in (0..4).flat_map(|r| (0..4).map(move |c| (r, c))) {
            let r_orig = r_band[[r, c]];
            let g_orig = g_band[[r, c]];
            let b_orig = b_band[[r, c]];

            let r_sharp = results[0][[r, c]];
            let g_sharp = results[1][[r, c]];
            let b_sharp = results[2][[r, c]];

            assert!(
                (r_sharp - r_orig).abs() < 1e-6,
                "R mismatch at [{r},{c}]: got {r_sharp}, expected {r_orig}"
            );
            assert!(
                (g_sharp - g_orig).abs() < 1e-6,
                "G mismatch at [{r},{c}]: got {g_sharp}, expected {g_orig}"
            );
            assert!(
                (b_sharp - b_orig).abs() < 1e-6,
                "B mismatch at [{r},{c}]: got {b_sharp}, expected {b_orig}"
            );
        }
    }

    /// Output shape must equal pan shape for a 3-band input.
    #[test]
    fn test_ihs_shape_invariant() {
        let ms = Array2::from_elem((5, 8), 0.5_f64);
        let pan = Array2::from_elem((5, 8), 0.8_f64);

        let transform = IHSPanSharpening;
        let results = transform
            .sharpen(&[ms.view(), ms.view(), ms.view()], &pan.view())
            .unwrap();

        assert_eq!(results.len(), 3);
        for result in &results {
            assert_eq!(result.dim(), (5, 8));
        }
    }
}

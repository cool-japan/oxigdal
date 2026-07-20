//! PCA (Principal Component Analysis) based pan-sharpening
//!
//! Implements the PC-substitution fusion method described in:
//! Chavez, P.S. & Kwarteng, A.Y. (1989) "Extracting spectral contrast in
//! Landsat Thematic Mapper image data using selective principal component
//! analysis." *Photogrammetric Engineering and Remote Sensing* 55(3), 339–348.
//!
//! Algorithm:
//! 1. Form pixel matrix X (n_pixels × n_bands), compute band means and subtract.
//! 2. Compute covariance C = X_centred^T · X_centred / (n − 1).
//! 3. Find PC1 eigenvector by power iteration (200 iterations).
//! 4. Project pixels onto PC1 → scores_pc1.
//! 5. Histogram-match Pan to scores_pc1.
//! 6. Replace PC1 scores with matched Pan; compute per-band correction
//!    and add back to the original pixel matrix.

use super::PanSharpening;
use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array2, ArrayView2};

/// PCA-based pan-sharpening.
pub struct PCAPanSharpening;

// ── internal helpers ────────────────────────────────────────────────────────

/// Power-iteration to find the dominant eigenvector of a square symmetric
/// matrix `m` (size `n × n`).
///
/// Returns `(eigenvalue, eigenvector)` after `max_iters` iterations.
/// `m` is provided as a flat row-major `Vec<f64>` of length `n * n`.
fn dominant_eigenvector(m: &[f64], n: usize, max_iters: usize) -> (f64, Vec<f64>) {
    // Initial guess: unit vector [1/√n, …].
    let init = 1.0 / (n as f64).sqrt();
    let mut v: Vec<f64> = vec![init; n];

    for _ in 0..max_iters {
        // w = M · v
        let mut w = vec![0.0_f64; n];
        for i in 0..n {
            for j in 0..n {
                w[i] += m[i * n + j] * v[j];
            }
        }
        // Normalise w.
        let norm: f64 = w.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm < 1e-14 {
            // All-zero result; matrix is degenerate — return v unchanged.
            break;
        }
        for i in 0..n {
            v[i] = w[i] / norm;
        }
    }

    // Eigenvalue λ = v^T · M · v
    let mut mv = vec![0.0_f64; n];
    for i in 0..n {
        for j in 0..n {
            mv[i] += m[i * n + j] * v[j];
        }
    }
    let eigenvalue: f64 = v.iter().zip(mv.iter()).map(|(vi, mvi)| vi * mvi).sum();

    (eigenvalue, v)
}

/// Linear histogram-match: rescale `src` values so they span the range of `ref_`.
fn histogram_match(src: &[f64], ref_: &[f64]) -> Vec<f64> {
    let src_min = src.iter().copied().fold(f64::INFINITY, f64::min);
    let src_max = src.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let ref_min = ref_.iter().copied().fold(f64::INFINITY, f64::min);
    let ref_max = ref_.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    let src_range = (src_max - src_min).max(1e-12);
    let ref_range = ref_max - ref_min;

    src.iter()
        .map(|&s| (s - src_min) / src_range * ref_range + ref_min)
        .collect()
}

// ── trait impl ──────────────────────────────────────────────────────────────

impl PanSharpening for PCAPanSharpening {
    fn sharpen(
        &self,
        ms_bands: &[ArrayView2<f64>],
        pan: &ArrayView2<f64>,
    ) -> Result<Vec<Array2<f64>>> {
        if ms_bands.is_empty() {
            return Err(SensorError::pan_sharpening_error(
                "PCAPanSharpening requires at least one MS band",
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
        let n_pixels = rows * cols;
        let n_bands = ms_bands.len();

        // ── 1. Build pixel matrix X (n_pixels × n_bands) ──────────────────
        // Flatten each band into a column; store in row-major order.
        // X[pixel * n_bands + band] = ms_bands[band][r, c]
        let mut x: Vec<f64> = vec![0.0; n_pixels * n_bands];
        for (b, band) in ms_bands.iter().enumerate() {
            let mut pixel = 0usize;
            for r in 0..rows {
                for c in 0..cols {
                    x[pixel * n_bands + b] = band[[r, c]];
                    pixel += 1;
                }
            }
        }

        // ── 2. Compute band means and centre X ────────────────────────────
        let mut means: Vec<f64> = vec![0.0; n_bands];
        for pixel in 0..n_pixels {
            for b in 0..n_bands {
                means[b] += x[pixel * n_bands + b];
            }
        }
        for mean in &mut means {
            *mean /= n_pixels as f64;
        }

        // X_centred in place.
        for pixel in 0..n_pixels {
            for b in 0..n_bands {
                x[pixel * n_bands + b] -= means[b];
            }
        }

        // ── Special case: single band — degenerate to histogram match ──────
        if n_bands == 1 {
            let ms_flat: Vec<f64> = ms_bands[0].iter().copied().collect();
            let pan_flat: Vec<f64> = pan.iter().copied().collect();
            let matched = histogram_match(&pan_flat, &ms_flat);
            let sharpened = Array2::from_shape_vec((rows, cols), matched)
                .map_err(|e| SensorError::pan_sharpening_error(format!("shape error: {e}")))?;
            return Ok(vec![sharpened]);
        }

        // ── 3. Covariance matrix C = X_centred^T · X_centred / (n-1) ──────
        // C is (n_bands × n_bands); stored row-major as flat Vec.
        let denom = (n_pixels.max(2) - 1) as f64;
        let mut cov: Vec<f64> = vec![0.0; n_bands * n_bands];

        for pixel in 0..n_pixels {
            let row_start = pixel * n_bands;
            for i in 0..n_bands {
                for j in 0..n_bands {
                    cov[i * n_bands + j] += x[row_start + i] * x[row_start + j];
                }
            }
        }
        for v in cov.iter_mut() {
            *v /= denom;
        }

        // ── 4. Find PC1 eigenvector (power iteration) ─────────────────────
        let (_lambda1, e1) = dominant_eigenvector(&cov, n_bands, 200);

        // ── 5. Project pixels onto PC1 ────────────────────────────────────
        // scores_pc1[pixel] = X_centred[pixel, :] · e1
        let mut scores_pc1: Vec<f64> = vec![0.0; n_pixels];
        for (pixel, score) in scores_pc1.iter_mut().enumerate() {
            let row_start = pixel * n_bands;
            for b in 0..n_bands {
                *score += x[row_start + b] * e1[b];
            }
        }

        // ── 6. Histogram-match Pan to scores_pc1 ─────────────────────────
        let pan_flat: Vec<f64> = pan.iter().copied().collect();
        let pan_matched = histogram_match(&pan_flat, &scores_pc1);

        // ── 7. Compute delta and back-project ─────────────────────────────
        // delta_pc1[pixel] = pan_matched[pixel] - scores_pc1[pixel]
        // correction[pixel, b] = delta_pc1[pixel] * e1[b]
        // sharpened[pixel, b] = x[pixel, b] + correction[pixel, b]
        //                     = (ms[pixel,b] - mean[b]) + mean[b] + delta * e1[b]
        //                     = ms[pixel,b] + delta * e1[b]
        //
        // Note: x is still the centred version.  Add back means and correction.
        let mut sharpened_flat: Vec<Vec<f64>> = (0..n_bands).map(|_| vec![0.0; n_pixels]).collect();

        for pixel in 0..n_pixels {
            let delta = pan_matched[pixel] - scores_pc1[pixel];
            let row_start = pixel * n_bands;
            for b in 0..n_bands {
                // x[..] is centred; add back mean and add correction.
                sharpened_flat[b][pixel] = x[row_start + b] + means[b] + delta * e1[b];
            }
        }

        // ── 8. Reshape to (rows × cols) per band ─────────────────────────
        let mut results: Vec<Array2<f64>> = Vec::with_capacity(n_bands);
        for band_data in &sharpened_flat {
            let arr = Array2::from_shape_vec((rows, cols), band_data.clone())
                .map_err(|e| SensorError::pan_sharpening_error(format!("shape error: {e}")))?;
            results.push(arr);
        }

        Ok(results)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::Array2;

    /// Output shape must equal pan shape.
    #[test]
    fn test_pca_shape_invariant() {
        let ms = Array2::from_elem((6, 5), 0.5_f64);
        let pan = Array2::from_elem((6, 5), 0.8_f64);

        let transform = PCAPanSharpening;
        let bands = [ms.view(), ms.view(), ms.view()];
        let results = transform.sharpen(&bands, &pan.view()).unwrap();

        assert_eq!(results.len(), 3);
        for result in &results {
            assert_eq!(result.dim(), (6, 5));
        }
    }

    /// Single-band case: the output is the histogram-matched pan.
    /// When ms is uniform (all 0.4) and pan varies, the matched pan should
    /// span the same statistical range as the ms band.
    #[test]
    fn test_pca_single_band() {
        let ms = Array2::from_elem((4, 4), 0.4_f64);
        // Pan varies between 0.2 and 0.9.
        let pan_data: Vec<f64> = (0..16).map(|i| 0.2 + i as f64 * (0.7 / 15.0)).collect();
        let pan = Array2::from_shape_vec((4, 4), pan_data.clone()).unwrap();

        let transform = PCAPanSharpening;
        let results = transform.sharpen(&[ms.view()], &pan.view()).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].dim(), (4, 4));

        // With uniform ms the matched output is constant at 0.4.
        for &v in results[0].iter() {
            assert!(
                (v - 0.4_f64).abs() < 1e-10,
                "Expected 0.4 for uniform ms single-band, got {v}"
            );
        }
    }

    /// Three-band uniform input: when all bands have the same constant value,
    /// the covariance is zero, power iteration converges to the initial vector
    /// [1/√3, 1/√3, 1/√3], and all sharpened bands should equal the
    /// histogram-matched pan broadcast across all bands.
    #[test]
    fn test_pca_three_band_uniform() {
        let val = 0.5_f64;
        let ms = Array2::from_elem((4, 4), val);

        // Pan with range [0.2, 0.8] — gives a non-trivial rescaling.
        let pan_data: Vec<f64> = (0..16).map(|i| 0.2 + i as f64 * (0.6 / 15.0)).collect();
        let pan = Array2::from_shape_vec((4, 4), pan_data).unwrap();

        let transform = PCAPanSharpening;
        let bands = [ms.view(), ms.view(), ms.view()];
        let results = transform.sharpen(&bands, &pan.view()).unwrap();

        assert_eq!(results.len(), 3);

        // All three sharpened bands must be identical (symmetry of uniform input).
        for pixel in 0..16 {
            let r = pixel / 4;
            let c = pixel % 4;
            let v0 = results[0][[r, c]];
            let v1 = results[1][[r, c]];
            let v2 = results[2][[r, c]];
            assert!(
                (v0 - v1).abs() < 1e-10,
                "Band 0 and 1 differ at [{r},{c}]: {v0} vs {v1}"
            );
            assert!(
                (v0 - v2).abs() < 1e-10,
                "Band 0 and 2 differ at [{r},{c}]: {v0} vs {v2}"
            );
        }
    }
}

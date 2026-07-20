//! Savitzky-Golay smoothing filter for NDVI time series.
//!
//! Implements the convolution-based Savitzky-Golay (SG) filter following:
//!
//! Savitzky, A., & Golay, M. J. E. (1964). Smoothing and differentiation of
//! data by simplified least squares procedures.
//! *Analytical Chemistry*, 36(8), 1627–1639.
//! <https://doi.org/10.1021/ac60214a047>
//!
//! The filter fits a polynomial of degree `poly_order` through a sliding window
//! of `window` equally-spaced points and evaluates that polynomial at the
//! centre of the window. This is equivalent to convolving the signal with a
//! fixed kernel that can be pre-computed from the Vandermonde least-squares
//! normal equations.
//!
//! ## Gap handling
//!
//! Savitzky-Golay is a smoother, not a gap detector. Gaps (NaN values) are
//! therefore pre-filled with linear interpolation before the convolution is
//! applied. The convolution result at interpolated positions is meaningful as a
//! smooth estimate, but callers that need to distinguish observed from
//! imputed values should keep track of the original NaN mask separately.
//!
//! ## Edge treatment
//!
//! At the left and right edges of the signal the full symmetric window cannot
//! be applied. The implementation uses a shrinking asymmetric window: for
//! position `i` the window extends from `max(0, i - half)` to
//! `min(n - 1, i + half)`, and a fresh set of Vandermonde least-squares
//! coefficients is computed for the actual (asymmetric) window size and the
//! evaluation position within that window.
//!
//! ## Computational cost
//!
//! O(n · window²) — polynomial fits for edge positions add some overhead over
//! the pure convolution O(n · window), but the total cost is dominated by the
//! convolution for practical NDVI series (`n ≈ 20–100`, `window ≈ 7–15`).

use scirs2_core::linalg::solve_ndarray;
use scirs2_core::ndarray::{Array1, Array2};

/// Apply a Savitzky-Golay smoothing filter to a 1-D signal `y`.
///
/// # Parameters
///
/// - `y` – Input signal; `f64::NAN` marks missing values and is pre-filled by
///   linear interpolation before the filter is applied.
/// - `window` – Length of the sliding window (must be odd; if even it is
///   silently incremented by one). Will be clamped to `[3, n]`.
/// - `poly_order` – Polynomial degree for the local least-squares fit. Must
///   be less than `window`; clamped to `window - 1` if necessary.
///
/// # Returns
///
/// The smoothed signal as a `Vec<f64>`. The length equals `y.len()`.
/// Returns an empty `Vec` when `y` is empty.
///
/// # Notes
///
/// The function never panics: edge cases (even `window`, small series,
/// degenerate Vandermonde systems) are handled by clamping or falling back to
/// a uniform moving-average kernel.
pub(crate) fn smooth_savgol(y: &[f64], window: usize, poly_order: usize) -> Vec<f64> {
    let n = y.len();
    if n == 0 {
        return Vec::new();
    }

    // ── Parameter validation and normalisation ────────────────────────────────
    // Force window to be odd.
    let window = if window.is_multiple_of(2) {
        window + 1
    } else {
        window
    };
    // Clamp window to [3, n].
    let window = window.max(3).min(n);
    // poly_order must be strictly less than window.
    let poly_order = poly_order.min(window - 1);
    let half = window / 2;

    // ── Pre-fill NaN gaps with linear interpolation ───────────────────────────
    let y_filled = pre_interpolate_nans(y);

    // ── Pre-compute the symmetric centre-window filter kernel ─────────────────
    let center_coeffs = compute_sg_kernel(window, poly_order, half);

    // ── Apply the filter, using asymmetric windows at the edges ───────────────
    let mut result = vec![0.0_f64; n];
    for i in 0..n {
        if i >= half && i + half < n {
            // Full symmetric window — use pre-computed kernel.
            result[i] = center_coeffs
                .iter()
                .zip(&y_filled[i - half..=i + half])
                .map(|(&c, &v)| c * v)
                .sum();
        } else {
            // Asymmetric edge window.
            let left = i.min(half);
            let right = (n - 1 - i).min(half);
            let actual_window = left + right + 1;
            let actual_poly = poly_order.min(actual_window - 1);
            let edge_kernel = compute_sg_kernel(actual_window, actual_poly, left);
            let start = i - left;
            result[i] = edge_kernel
                .iter()
                .zip(&y_filled[start..start + actual_window])
                .map(|(&c, &v)| c * v)
                .sum();
        }
    }

    result
}

// ─────────────────────────────────────────────────────────────────────────────
// Private helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Compute the SG filter kernel of length `window` for a polynomial of degree
/// `poly_order`, where the polynomial is evaluated at position `eval_pos`
/// within the window (0-indexed from the left).
///
/// The kernel `h` satisfies:
///
/// ```text
///   ẑ[eval_pos] = Σᵢ h[i] · y[i]   for i in 0..window
/// ```
///
/// ## Derivation (hat-matrix row via normal equations)
///
/// Let `A` be the Vandermonde matrix (`window × (poly_order+1)`) with
/// x-positions centred at `half = window/2`:
///
/// ```text
///   A[i, j] = (i - half)ʲ
/// ```
///
/// The OLS polynomial coefficient estimate is `β = (AᵀA)⁻¹Aᵀy`, and the
/// smoothed value at `eval_pos` is
///
/// ```text
///   ẑ = eᵀ · β = eᵀ (AᵀA)⁻¹ Aᵀ y
/// ```
///
/// where `e = A[eval_pos, :]` is the `(poly_order+1)`-dimensional Vandermonde
/// row for the evaluation x-position.
///
/// The filter kernel is therefore `h = A · (AᵀA)⁻¹ · e`. We compute it via
/// the normal equations:
///
/// 1. Solve the square `(poly_order+1) × (poly_order+1)` system
///    `(AᵀA) · β̃ = e` using `solve_ndarray` (exact solve, not lstsq).
/// 2. Multiply `h = A · β̃` to obtain the `window`-length kernel.
///
/// Note: `lstsq_ndarray` is intentionally **not** used here because the
/// underlying LAPACK backend rejects underdetermined systems (which would arise
/// if we tried to solve `Aᵀ · c = e` directly — `Aᵀ` is `(poly+1) × window`,
/// i.e., wide when `window > poly+1`).
///
/// Falls back to a uniform (`1/window`) kernel on any numerical failure.
fn compute_sg_kernel(window: usize, poly_order: usize, eval_pos: usize) -> Vec<f64> {
    if window == 0 {
        return Vec::new();
    }
    let half = window / 2;

    // ── Build Vandermonde design matrix A : window × (poly_order+1) ──────────
    let mut a = Array2::<f64>::zeros((window, poly_order + 1));
    for i in 0..window {
        let x = i as f64 - half as f64;
        for j in 0..=poly_order {
            a[[i, j]] = x.powi(j as i32);
        }
    }

    // ── Vandermonde row for the evaluation position ───────────────────────────
    let eval_x = eval_pos as f64 - half as f64;
    let e: Array1<f64> = Array1::from_iter((0..=poly_order).map(|j| eval_x.powi(j as i32)));

    // ── Step 1: form the normal-equation matrix AᵀA (square, (poly+1)×(poly+1)) ──
    let at = a.t().to_owned(); // (poly_order+1) × window
    let ata: Array2<f64> = at.dot(&a); // (poly_order+1) × (poly_order+1)

    // ── Step 2: solve (AᵀA) · beta_tilde = e  (square system, exact solve) ───
    let beta_tilde = match solve_ndarray(&ata, &e) {
        Ok(beta) => beta,
        Err(_) => return vec![1.0 / window as f64; window],
    };

    // ── Step 3: h = A · beta_tilde  (window-length kernel) ───────────────────
    let h = a.dot(&beta_tilde);
    h.to_vec()
}

/// Pre-interpolate NaN values in `y` by piecewise linear interpolation between
/// the nearest valid (non-NaN) anchor points.
///
/// - Leading NaNs (before the first valid observation) are filled with the
///   first valid value (nearest-neighbour extrapolation).
/// - Trailing NaNs (after the last valid observation) are filled with the last
///   valid value (nearest-neighbour extrapolation).
/// - Interior NaN runs are filled by linear interpolation between the
///   surrounding valid anchor points.
/// - If the entire series is NaN a vector of zeros is returned.
fn pre_interpolate_nans(y: &[f64]) -> Vec<f64> {
    let n = y.len();
    let mut result = y.to_vec();

    // Collect indices of all valid (non-NaN) observations.
    let valid: Vec<usize> = (0..n).filter(|&i| !y[i].is_nan()).collect();

    if valid.is_empty() {
        return vec![0.0; n];
    }

    // ── Fill leading NaNs ─────────────────────────────────────────────────────
    let first_valid = valid[0];
    let fill_leading = y[first_valid];
    result[..first_valid]
        .iter_mut()
        .for_each(|v| *v = fill_leading);

    // ── Fill trailing NaNs ────────────────────────────────────────────────────
    let last_valid = valid[valid.len() - 1]; // valid is non-empty (checked above)
    let fill_trailing = y[last_valid];
    result[(last_valid + 1)..]
        .iter_mut()
        .for_each(|v| *v = fill_trailing);

    // ── Fill interior NaN runs by linear interpolation ────────────────────────
    for w in valid.windows(2) {
        let (i0, i1) = (w[0], w[1]);
        if i1 > i0 + 1 {
            let v0 = y[i0];
            let v1 = y[i1];
            for (offset, slot) in result[(i0 + 1)..i1].iter_mut().enumerate() {
                let t = (offset + 1) as f64 / (i1 - i0) as f64;
                *slot = v0 + t * (v1 - v0);
            }
        }
    }

    result
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        assert!(smooth_savgol(&[], 7, 2).is_empty());
    }

    #[test]
    fn constant_series_reproduced() {
        let y = vec![3.0_f64; 30];
        let z = smooth_savgol(&y, 7, 2);
        for (&zi, &yi) in z.iter().zip(y.iter()) {
            assert!(
                (zi - yi).abs() < 1e-6,
                "constant not reproduced: got {zi}, expected {yi}"
            );
        }
    }

    #[test]
    fn even_window_bumped_to_odd() {
        // Must not panic and must return a Vec of the same length.
        let y: Vec<f64> = (0..20).map(|i| i as f64).collect();
        let z = smooth_savgol(&y, 6, 2); // 6 → 7
        assert_eq!(z.len(), y.len());
        assert!(z.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn pre_interpolate_all_nan_returns_zeros() {
        let y = vec![f64::NAN; 5];
        let result = pre_interpolate_nans(&y);
        assert_eq!(result, vec![0.0; 5]);
    }

    #[test]
    fn pre_interpolate_leading_and_trailing() {
        let y = vec![f64::NAN, f64::NAN, 4.0, 8.0, f64::NAN];
        let result = pre_interpolate_nans(&y);
        assert_eq!(result[0], 4.0); // leading NaN → first valid
        assert_eq!(result[1], 4.0);
        assert_eq!(result[2], 4.0);
        assert_eq!(result[3], 8.0);
        assert_eq!(result[4], 8.0); // trailing NaN → last valid
    }

    #[test]
    fn kernel_length_matches_window() {
        let kernel = compute_sg_kernel(7, 2, 3);
        assert_eq!(kernel.len(), 7);
    }

    #[test]
    fn kernel_sums_to_one_for_constant_polynomial() {
        // For poly_order >= 0 the constant is reproduced, so coefficients sum to 1.
        let kernel = compute_sg_kernel(7, 2, 3);
        let sum: f64 = kernel.iter().sum();
        assert!((sum - 1.0).abs() < 1e-10, "kernel sum = {sum}");
    }
}

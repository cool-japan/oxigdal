//! Whittaker smoother for NDVI time series.
//!
//! Implements the Whittaker smoother as described in:
//!
//! Eilers, P. H. C. (2003). A perfect smoother.
//! *Analytical Chemistry*, 75(14), 3631–3636.
//! <https://doi.org/10.1021/ac034173t>
//!
//! The smoother solves the penalised least-squares system
//!
//! ```text
//!   (W + λ·Dᵀ·D)·z = W·y
//! ```
//!
//! where
//! - `y` is the observed signal (NaN encodes missing / gap observations),
//! - `W = diag(w)` with `w_i = 0` at NaN and `w_i = 1` otherwise,
//! - `D` is the order-*d* finite-difference operator (`(n-d) × n`),
//! - `λ ≥ 0` controls the smoothness penalty, and
//! - `z` is the smooth estimate.
//!
//! Because the weight of a missing observation is zero its residual does not
//! contribute to the cost; the smoother therefore fills the gap with the
//! smoothly interpolated value implied by the surrounding observations.
//!
//! The linear system is solved with [`scirs2_core::linalg::solve_ndarray`].
//! For typical NDVI time series (`n ≈ 20–100`) the O(n³) dense solve is
//! perfectly acceptable and avoids the band-matrix bookkeeping that would be
//! required for an O(n) solver.

use scirs2_core::linalg::solve_ndarray;
use scirs2_core::ndarray::{Array1, Array2};

/// Apply the Whittaker smoother to a 1-D signal `y`.
///
/// # Parameters
///
/// - `y` – Input signal; `f64::NAN` marks missing / masked observations.
/// - `lambda` – Smoothness penalty weight (λ). Larger values produce a
///   smoother output. Typical NDVI values range from 10 to 10 000; the Eilers
///   (2003) paper uses λ = 1 600 as a starting point.
/// - `order` – Order of the finite-difference penalty (1 = first differences,
///   2 = second differences). Order 2 is recommended for vegetation index
///   series because it penalises curvature, preserving linear trends.
///
/// # Returns
///
/// The smoothed signal as a `Vec<f64>`. Positions that were NaN in `y` are
/// filled by the smooth interpolant; all other positions are updated to the
/// smooth estimate.
///
/// # Fall-backs
///
/// - If `y` is empty, or if `n ≤ order` (system is under-determined), the
///   function returns a clone of `y` unchanged.
/// - If all values in `y` are NaN the function returns a clone of `y`
///   unchanged (nothing to anchor the smoother).
/// - If the linear system is numerically singular (extremely unlikely in
///   practice when `λ > 0`) the function returns a clone of `y` unchanged.
pub(crate) fn smooth_whittaker(y: &[f64], lambda: f64, order: usize) -> Vec<f64> {
    let n = y.len();
    if n == 0 || n <= order {
        return y.to_vec();
    }

    // ── Weight vector: 0 at NaN / missing, 1 at valid observations ──────────
    let w: Vec<f64> = y
        .iter()
        .map(|&v| if v.is_nan() { 0.0 } else { 1.0 })
        .collect();

    // If every observation is missing there is nothing to anchor the fit.
    if w.iter().all(|&wi| wi == 0.0) {
        return y.to_vec();
    }

    // Replace NaN with 0 in the right-hand side; the weight matrix zeros out
    // the contribution of those positions anyway.
    let y_clean: Array1<f64> =
        Array1::from_iter(y.iter().map(|&v| if v.is_nan() { 0.0 } else { v }));

    // ── Build order-d finite-difference operator D : (n-d) × n ──────────────
    let d = build_difference_matrix(n, order);

    // ── Build diagonal weight matrix W : n × n ───────────────────────────────
    let mut w_mat = Array2::<f64>::zeros((n, n));
    for i in 0..n {
        w_mat[[i, i]] = w[i];
    }

    // ── Assemble A = W + λ·Dᵀ·D,  b = W·y ───────────────────────────────────
    let dtd = d.t().dot(&d);
    let a: Array2<f64> = w_mat + lambda * dtd;
    let b: Array1<f64> = Array1::from_iter((0..n).map(|i| w[i] * y_clean[i]));

    // ── Solve the linear system ───────────────────────────────────────────────
    match solve_ndarray(&a, &b) {
        Ok(z) => z.to_vec(),
        Err(_) => y.to_vec(), // fall back on rare singular matrix
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Private helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build the order-*d* finite-difference operator matrix `D` of shape
/// `(n - d) × n` by repeated left-multiplication with the first-difference
/// operator.
///
/// Special cases:
/// - `order == 0` → identity matrix (no penalty on changes, smoother reduces
///   to weighted mean at every point independently).
/// - `n <= order` → identity (fall-back; caller should have returned early).
fn build_difference_matrix(n: usize, order: usize) -> Array2<f64> {
    if order == 0 || n <= order {
        return Array2::eye(n);
    }
    let mut d = build_first_difference(n);
    for _ in 1..order {
        let rows = d.nrows();
        let d1 = build_first_difference(rows);
        d = d1.dot(&d);
    }
    d
}

/// Build the first-difference operator matrix of shape `(n-1) × n`:
///
/// ```text
///  D₁[i, i]   = -1
///  D₁[i, i+1] =  1
/// ```
fn build_first_difference(n: usize) -> Array2<f64> {
    let mut d = Array2::<f64>::zeros((n - 1, n));
    for i in 0..n - 1 {
        d[[i, i]] = -1.0;
        d[[i, i + 1]] = 1.0;
    }
    d
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn constant_series_reproduced_exactly() {
        let y = vec![5.0_f64; 20];
        let z = smooth_whittaker(&y, 100.0, 2);
        for (&zi, &yi) in z.iter().zip(y.iter()) {
            assert!(
                (zi - yi).abs() < 1e-8,
                "constant series not reproduced: got {zi}, expected {yi}"
            );
        }
    }

    #[test]
    fn empty_input_returns_empty() {
        let z = smooth_whittaker(&[], 100.0, 2);
        assert!(z.is_empty());
    }

    #[test]
    fn short_series_at_boundary_falls_back() {
        // n == order → fall-back
        let y = vec![1.0, 2.0]; // n = 2 == order = 2
        let z = smooth_whittaker(&y, 100.0, 2);
        assert_eq!(z, y);
    }

    #[test]
    fn all_nan_returns_input() {
        let y = vec![f64::NAN; 10];
        let z = smooth_whittaker(&y, 100.0, 2);
        assert_eq!(z.len(), y.len());
        assert!(z.iter().all(|v| v.is_nan()));
    }

    #[test]
    fn build_first_difference_shape() {
        let d = build_first_difference(5);
        assert_eq!(d.nrows(), 4);
        assert_eq!(d.ncols(), 5);
    }

    #[test]
    fn build_difference_matrix_order2_shape() {
        let d = build_difference_matrix(10, 2);
        assert_eq!(d.nrows(), 8); // n - order = 10 - 2
        assert_eq!(d.ncols(), 10);
    }
}

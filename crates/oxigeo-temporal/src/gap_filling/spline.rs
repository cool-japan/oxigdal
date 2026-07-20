//! Natural cubic spline gap filling for pixel time series.
//!
//! Implements the classic *natural* cubic spline (Burden & Faires, *Numerical
//! Analysis*, §3.5): a piecewise cubic polynomial that interpolates the
//! observed (non-NaN) samples, is `C²` continuous at every knot, and has zero
//! curvature (`S''(x) = 0`) at the two boundary knots.
//!
//! Unlike `GapFiller::interpolate_linear`, which fills each gap with
//! a straight line between its two immediate neighbours, the spline uses
//! *all* valid anchor points to determine the curvature of the fitted curve,
//! producing a smoothly-varying (rather than piecewise-linear) fill.
//!
//! ## Algorithm
//!
//! Given knots `x_0 < x_1 < … < x_{m-1}` (the indices of the valid, non-NaN
//! samples) with values `y_0, …, y_{m-1}`, the natural cubic spline's second
//! derivatives `M_0, …, M_{m-1}` at the knots satisfy the tridiagonal system
//!
//! ```text
//!   M_0 = M_{m-1} = 0                                            (natural BC)
//!   h_{i-1}·M_{i-1} + 2·(h_{i-1}+h_i)·M_i + h_i·M_{i+1}
//!       = 6·[(y_{i+1}-y_i)/h_i - (y_i-y_{i-1})/h_{i-1}]     for i = 1..m-2
//! ```
//!
//! where `h_i = x_{i+1} - x_i`. This system is solved once per pixel
//! timeseries via [`scirs2_core::linalg::tridiag_solve_ndarray`] (the same
//! LAPACK-backed tridiagonal solver family used elsewhere in this crate), and
//! each interior gap is then evaluated with the standard cubic Hermite-style
//! spline formula on its containing segment.
//!
//! Gaps outside the range of the first/last valid anchor (leading/trailing
//! NaNs) are left unfilled, matching the behaviour of
//! `GapFiller::interpolate_linear`.

use scirs2_core::linalg::tridiag_solve_ndarray;
use scirs2_core::ndarray::Array1;

/// Fill NaN gaps in `values` with a natural cubic spline through the valid
/// (non-NaN) samples.
///
/// Returns a `Vec` of the same length as `values`. Already-valid samples are
/// passed through unchanged; NaN samples strictly between the first and last
/// valid sample are replaced with the spline's fitted value; NaN samples
/// outside that range (leading/trailing) are left as NaN, since there is
/// nothing to interpolate between.
///
/// Never panics: degenerate inputs (fewer than 2 valid anchors, duplicate
/// knot positions, or a singular tridiagonal system) fall back to leaving the
/// series unfilled (fewer than 2 anchors) or to a zero-curvature spline
/// (equivalent to the linear interpolant) rather than erroring.
pub(crate) fn fill_natural_cubic_spline(values: &[f64]) -> Vec<f64> {
    let mut result = values.to_vec();

    let anchors: Vec<(usize, f64)> = values
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.is_nan())
        .map(|(i, &v)| (i, v))
        .collect();

    let n_anchor = anchors.len();
    if n_anchor < 2 {
        // Not enough anchors to interpolate anything; mirror interpolate_linear.
        return result;
    }

    let xs: Vec<f64> = anchors.iter().map(|&(i, _)| i as f64).collect();
    let ys: Vec<f64> = anchors.iter().map(|&(_, v)| v).collect();

    let m = second_derivatives(&xs, &ys);

    for (i, slot) in result.iter_mut().enumerate() {
        if !slot.is_nan() {
            continue;
        }
        let x = i as f64;
        if x < xs[0] || x > xs[n_anchor - 1] {
            continue; // outside the anchor range: no extrapolation, matches linear fill
        }

        if let Some(seg) = xs.windows(2).position(|w| x >= w[0] && x <= w[1]) {
            *slot = evaluate_cubic(&xs, &ys, &m, seg, x);
        }
    }

    result
}

/// Solve the natural cubic spline's tridiagonal system for the second
/// derivatives `M` at each knot `(xs[i], ys[i])`.
///
/// `M[0]` and `M[xs.len() - 1]` are always `0.0` (natural boundary
/// condition). Falls back to an all-zero curvature vector (which reduces the
/// spline to a piecewise-linear interpolant) when the knot count is too small
/// to form interior equations, when knot spacing is non-positive
/// (duplicate/unsorted x-coordinates), or when the tridiagonal solve fails.
fn second_derivatives(xs: &[f64], ys: &[f64]) -> Vec<f64> {
    let n = xs.len();
    if n < 3 {
        return vec![0.0; n];
    }

    let h: Vec<f64> = xs.windows(2).map(|w| w[1] - w[0]).collect();
    if h.iter().any(|&hi| hi <= 0.0) {
        return vec![0.0; n];
    }

    let n_interior = n - 2;
    let mut dl = vec![0.0_f64; n_interior.saturating_sub(1)];
    let mut d = vec![0.0_f64; n_interior];
    let mut du = vec![0.0_f64; n_interior.saturating_sub(1)];
    let mut rhs = vec![0.0_f64; n_interior];

    for i in 0..n_interior {
        // Interior knot global index = i + 1; h_im1 = h[i], h_i = h[i + 1].
        let h_im1 = h[i];
        let h_i = h[i + 1];
        d[i] = 2.0 * (h_im1 + h_i);
        rhs[i] = 6.0 * ((ys[i + 2] - ys[i + 1]) / h_i - (ys[i + 1] - ys[i]) / h_im1);
        if i > 0 {
            dl[i - 1] = h_im1;
        }
        if i + 1 < n_interior {
            du[i] = h_i;
        }
    }

    let dl = Array1::from_vec(dl);
    let d = Array1::from_vec(d);
    let du = Array1::from_vec(du);
    let rhs = Array1::from_vec(rhs);

    let m_interior = match tridiag_solve_ndarray(&dl, &d, &du, &rhs) {
        Ok(m) => m,
        Err(_) => return vec![0.0; n],
    };

    let mut m = vec![0.0; n];
    for i in 0..n_interior {
        m[i + 1] = m_interior[i];
    }
    m
}

/// Evaluate the natural cubic spline on segment `[xs[seg], xs[seg + 1]]` at
/// position `x`, given the knot second derivatives `m`.
fn evaluate_cubic(xs: &[f64], ys: &[f64], m: &[f64], seg: usize, x: f64) -> f64 {
    let x0 = xs[seg];
    let x1 = xs[seg + 1];
    let h = x1 - x0;
    if h <= 0.0 {
        return ys[seg];
    }

    let a = (x1 - x) / h;
    let b = (x - x0) / h;
    a * ys[seg]
        + b * ys[seg + 1]
        + ((a.powi(3) - a) * m[seg] + (b.powi(3) - b) * m[seg + 1]) * (h * h) / 6.0
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod unit_tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        assert!(fill_natural_cubic_spline(&[]).is_empty());
    }

    #[test]
    fn all_nan_stays_nan() {
        let y = vec![f64::NAN; 5];
        let result = fill_natural_cubic_spline(&y);
        assert!(result.iter().all(|v| v.is_nan()));
    }

    #[test]
    fn single_anchor_stays_unfilled() {
        let mut y = vec![f64::NAN; 5];
        y[2] = 3.0;
        let result = fill_natural_cubic_spline(&y);
        assert_eq!(result[2], 3.0);
        assert!(result[0].is_nan());
        assert!(result[4].is_nan());
    }

    #[test]
    fn two_anchors_reduce_to_a_line() {
        let y = vec![0.0, f64::NAN, f64::NAN, f64::NAN, 12.0];
        let result = fill_natural_cubic_spline(&y);
        for (i, &v) in result.iter().enumerate() {
            assert!((v - 3.0 * i as f64).abs() < 1e-9, "index {i}: got {v}");
        }
    }

    #[test]
    fn leading_and_trailing_nan_left_unfilled() {
        let y = vec![f64::NAN, 1.0, 2.0, 3.0, f64::NAN];
        let result = fill_natural_cubic_spline(&y);
        assert!(result[0].is_nan());
        assert!(result[4].is_nan());
        assert_eq!(result[1], 1.0);
    }

    #[test]
    fn valid_samples_pass_through_unchanged() {
        let y = vec![1.0, f64::NAN, 3.0, f64::NAN, 5.0];
        let result = fill_natural_cubic_spline(&y);
        assert_eq!(result[0], 1.0);
        assert_eq!(result[2], 3.0);
        assert_eq!(result[4], 5.0);
    }

    #[test]
    fn quadratic_series_recovered_with_high_accuracy() {
        // A cubic spline should recover a smooth quadratic near-exactly at
        // interior gaps, unlike piecewise-linear interpolation.
        let n = 11;
        let full: Vec<f64> = (0..n).map(|i| (i as f64 - 5.0).powi(2)).collect();
        let mut gappy = full.clone();
        for i in [2usize, 3, 7, 8] {
            gappy[i] = f64::NAN;
        }

        let spline_result = fill_natural_cubic_spline(&gappy);
        for i in [2usize, 3, 7, 8] {
            assert!(
                (spline_result[i] - full[i]).abs() < 0.5,
                "index {i}: spline {} vs truth {}",
                spline_result[i],
                full[i]
            );
        }
    }

    #[test]
    fn differs_from_piecewise_linear_on_curved_signal() {
        // Sample a sine wave with a gap spanning several points; the spline
        // fill should differ measurably from a straight-line (linear) fill
        // across the gap, since the true signal has curvature.
        let n = 20;
        let full: Vec<f64> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * i as f64 / n as f64).sin())
            .collect();
        let mut gappy = full.clone();
        for v in gappy.iter_mut().take(12).skip(7) {
            *v = f64::NAN;
        }

        let spline_result = fill_natural_cubic_spline(&gappy);

        // Linear interpolation across the same gap (straight line between
        // the two boundary anchors at indices 6 and 12).
        let (i0, i1) = (6usize, 12usize);
        let (v0, v1) = (full[i0], full[i1]);
        let linear_at = |i: usize| v0 + (v1 - v0) * ((i - i0) as f64 / (i1 - i0) as f64);

        let mut max_diff = 0.0_f64;
        for (i, &sv) in spline_result.iter().enumerate().take(i1).skip(i0 + 1) {
            let diff = (sv - linear_at(i)).abs();
            max_diff = max_diff.max(diff);
        }
        assert!(
            max_diff > 0.05,
            "spline fill should visibly differ from linear fill on a curved signal, max_diff={max_diff}"
        );
    }
}

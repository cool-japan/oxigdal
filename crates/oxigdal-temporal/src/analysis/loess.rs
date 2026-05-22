//! Loess (Locally Estimated Scatterplot Smoothing)
//!
//! This module implements 1D Loess smoothing following Cleveland (1979) and
//! the description given in Cleveland & Devlin (1988) and Cleveland (1990).
//!
//! Loess fits a low-degree weighted polynomial at every target abscissa using
//! the `k` nearest neighbours, where `k = ceil(bandwidth_fraction * n)`. The
//! weights are produced by the tri-cube kernel of equation (2) in Cleveland
//! (1979). When the local design matrix is rank-deficient (e.g. all weights
//! collapse to a single point), the implementation falls back to the
//! weighted mean of the local response, matching the convention used by
//! `statsmodels` and R's `loess` for degenerate neighbourhoods.
//!
//! # References
//!
//! - Cleveland, W. S. (1979). Robust locally weighted regression and smoothing
//!   scatterplots. *Journal of the American Statistical Association*, 74(368),
//!   829-836.
//! - Cleveland, W. S., & Devlin, S. J. (1988). Locally weighted regression: An
//!   approach to regression analysis by local fitting. *JASA*, 83(403), 596-610.
//! - Cleveland, R. B., Cleveland, W. S., McRae, J. E., & Terpenning, I. (1990).
//!   STL: A seasonal-trend decomposition procedure based on Loess.
//!   *Journal of Official Statistics*, 6(1), 3-73.

use crate::error::{Result, TemporalError};
use scirs2_core::linalg::solve_ndarray;
use scirs2_core::ndarray::{Array1, Array2};

/// Configuration options for 1D Loess smoothing.
#[derive(Debug, Clone)]
pub struct LoessOptions {
    /// Fraction of points (in `[0, 1]`) used in each local fit. The default is
    /// `2/3` as in Cleveland (1979).
    pub bandwidth_fraction: f64,
    /// Polynomial degree of the local fits (0, 1 or 2). The default is 1, the
    /// canonical "local linear" Loess.
    pub degree: u8,
    /// Number of bisquare robustness iterations to apply (Cleveland 1979 §6).
    /// Defaults to 0 (non-robust fit).
    pub robustness_iterations: usize,
    /// Optional pre-multiplicative weights applied alongside the tri-cube
    /// kernel. Useful for STL outer-loop robustness, where these come from the
    /// bisquare weights of the residuals.
    pub weights: Option<Vec<f64>>,
}

impl Default for LoessOptions {
    fn default() -> Self {
        Self {
            bandwidth_fraction: 2.0 / 3.0,
            degree: 1,
            robustness_iterations: 0,
            weights: None,
        }
    }
}

impl LoessOptions {
    /// Construct a new options struct with the supplied bandwidth fraction.
    #[must_use]
    pub fn new(bandwidth_fraction: f64) -> Self {
        Self {
            bandwidth_fraction,
            ..Self::default()
        }
    }

    /// Set the local polynomial degree.
    #[must_use]
    pub fn with_degree(mut self, degree: u8) -> Self {
        self.degree = degree;
        self
    }

    /// Set the number of robustness iterations.
    #[must_use]
    pub fn with_robustness_iterations(mut self, iterations: usize) -> Self {
        self.robustness_iterations = iterations;
        self
    }

    /// Provide explicit pre-multiplicative weights.
    #[must_use]
    pub fn with_weights(mut self, weights: Vec<f64>) -> Self {
        self.weights = Some(weights);
        self
    }
}

/// Loess-smooth `y` against abscissas `x` according to `options`.
///
/// `x` must be strictly increasing or at least monotonically non-decreasing —
/// no sorting is performed because the principal use case is an evenly spaced
/// time index.
///
/// # Errors
/// Returns an error when the inputs have mismatched lengths or when fewer
/// than two points are supplied.
pub fn loess_smooth_1d(x: &[f64], y: &[f64], options: &LoessOptions) -> Result<Vec<f64>> {
    if x.len() != y.len() {
        return Err(TemporalError::invalid_parameter(
            "x/y",
            format!(
                "length mismatch: x has {} elements, y has {}",
                x.len(),
                y.len()
            ),
        ));
    }
    if let Some(w) = &options.weights {
        if w.len() != y.len() {
            return Err(TemporalError::invalid_parameter(
                "weights",
                format!(
                    "length mismatch: weights have {} elements, y has {}",
                    w.len(),
                    y.len()
                ),
            ));
        }
    }
    let n = y.len();
    if n < 2 {
        return Err(TemporalError::insufficient_data(
            "Loess requires at least two points",
        ));
    }

    // Compute the neighbourhood size k = ceil(bandwidth_fraction * n), clamped
    // to [1, n]. A fraction of zero collapses the local fit to a constant at
    // the target, which is exposed as an explicit fallback below.
    let frac = options.bandwidth_fraction.clamp(0.0, 1.0);
    let k = ((frac * n as f64).ceil() as usize).clamp(1, n);

    // Bisquare robustness weights, computed iteratively (Cleveland 1979 §6).
    let mut robustness = vec![1.0_f64; n];

    let mut fitted = vec![0.0_f64; n];

    for iter in 0..=options.robustness_iterations {
        for (i, &target) in x.iter().enumerate() {
            fitted[i] = fit_at_target(
                x,
                y,
                target,
                k,
                &options.weights,
                &robustness,
                options.degree,
            );
        }
        if iter < options.robustness_iterations {
            // Update robustness weights from current residuals.
            let residuals: Vec<f64> = y
                .iter()
                .zip(fitted.iter())
                .map(|(yi, fi)| yi - fi)
                .collect();
            let mut abs_residuals: Vec<f64> = residuals.iter().map(|r| r.abs()).collect();
            // Median of absolute residuals via partial sort.
            abs_residuals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let median = if abs_residuals.is_empty() {
                0.0
            } else {
                abs_residuals[abs_residuals.len() / 2]
            };
            let scale = 6.0 * median;
            for (idx, r) in residuals.iter().enumerate() {
                robustness[idx] = if scale <= 0.0 {
                    1.0
                } else {
                    let u = r / scale;
                    if u.abs() >= 1.0 {
                        0.0
                    } else {
                        let v = 1.0 - u * u;
                        (v * v).clamp(0.0, 1.0)
                    }
                };
            }
        }
    }

    Ok(fitted)
}

/// Convenience entry-point that smooths a regularly indexed series with a
/// neighbourhood of `window` points. This is the form invoked by STL for both
/// cycle-subseries and trend smoothing — see Cleveland et al. (1990) §3.5.
#[must_use]
pub fn loess_smooth_indexed(values: &[f64], window: usize, degree: u8) -> Vec<f64> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return values.to_vec();
    }
    let k = window.max(1).min(n);
    let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let weights = vec![1.0_f64; n];
    let robustness = vec![1.0_f64; n];
    let mut out = Vec::with_capacity(n);
    for (i, &target) in xs.iter().enumerate() {
        let _ = i;
        out.push(fit_at_target(
            &xs,
            values,
            target,
            k,
            &Some(weights.clone()),
            &robustness,
            degree,
        ));
    }
    out
}

fn fit_at_target(
    x: &[f64],
    y: &[f64],
    target: f64,
    k: usize,
    weights: &Option<Vec<f64>>,
    robustness: &[f64],
    degree: u8,
) -> f64 {
    let n = x.len();
    // Identify the `k` nearest neighbours by absolute distance from target.
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&a, &b| {
        let da = (x[a] - target).abs();
        let db = (x[b] - target).abs();
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });
    let neighbour_count = k.min(n);
    let neighbour_idx = &indices[..neighbour_count];

    // Bandwidth = distance to the furthest neighbour, the "h" of Cleveland §2.
    // When k == n the bandwidth is the maximum distance over the data set.
    let mut max_dist = 0.0_f64;
    for &i in neighbour_idx {
        let d = (x[i] - target).abs();
        if d > max_dist {
            max_dist = d;
        }
    }
    let bandwidth = if max_dist == 0.0 { 1.0 } else { max_dist };

    // Compose tri-cube * (optional explicit) * robustness weights.
    let mut local_x = Vec::with_capacity(neighbour_count);
    let mut local_y = Vec::with_capacity(neighbour_count);
    let mut local_w = Vec::with_capacity(neighbour_count);
    let mut total_weight = 0.0_f64;
    for &i in neighbour_idx {
        let tri = tricube_weight((x[i] - target).abs(), bandwidth);
        let explicit = weights.as_ref().map(|ws| ws[i]).unwrap_or(1.0).max(0.0);
        let robust = robustness[i].max(0.0);
        let w = tri * explicit * robust;
        local_x.push(x[i]);
        local_y.push(y[i]);
        local_w.push(w);
        total_weight += w;
    }

    if total_weight <= 0.0 {
        // Fall back to the unweighted mean if every neighbour ended up with
        // zero weight (e.g. all explicit weights vanished after robustness
        // iterations).
        return local_y.iter().sum::<f64>() / local_y.len() as f64;
    }

    weighted_polynomial_fit_local(&local_x, &local_y, &local_w, degree, target)
}

/// Tri-cube weight kernel from Cleveland (1979) equation (2).
///
/// `tricube(d, h) = (1 - (d/h)^3)^3` for `d < h`, otherwise zero.
#[must_use]
pub fn tricube_weight(distance: f64, bandwidth: f64) -> f64 {
    if bandwidth <= 0.0 {
        return if distance == 0.0 { 1.0 } else { 0.0 };
    }
    let u = (distance / bandwidth).abs();
    if u >= 1.0 {
        0.0
    } else {
        let cube = 1.0 - u * u * u;
        let res = cube * cube * cube;
        res.max(0.0)
    }
}

/// Fit a weighted polynomial of `degree` to `(xs, ys, weights)` and evaluate
/// at `target`. Implements the local design described in Cleveland (1979) §3.
///
/// If the local design matrix is rank-deficient (e.g. only one distinct
/// abscissa, all weights collapsed) the routine falls back to the weighted
/// mean of the responses. This matches the behaviour described in Cleveland
/// (1979) §3.2 for "degenerate neighbourhoods".
#[must_use]
pub fn weighted_polynomial_fit_local(
    xs: &[f64],
    ys: &[f64],
    weights: &[f64],
    degree: u8,
    target: f64,
) -> f64 {
    let n = xs.len();
    if n == 0 {
        return 0.0;
    }
    let total_w: f64 = weights.iter().copied().filter(|w| *w > 0.0).sum();
    if total_w <= 0.0 {
        return ys.iter().sum::<f64>() / n as f64;
    }
    // Compute the weighted mean as the universal fallback / degree-0 result.
    let weighted_mean: f64 = xs
        .iter()
        .zip(ys.iter())
        .zip(weights.iter())
        .map(|((_, y), w)| w * y)
        .sum::<f64>()
        / total_w;
    if degree == 0 {
        return weighted_mean;
    }

    // Centre abscissas at the target — this improves conditioning of the
    // normal equations and is the form used in Cleveland (1979) eq. (3).
    let dx: Vec<f64> = xs.iter().map(|x| x - target).collect();

    // Check for rank deficiency: we need at least `degree + 1` neighbours with
    // strictly positive weight and at least two distinct centred abscissas.
    let active_count = weights.iter().filter(|w| **w > 0.0).count();
    if active_count <= degree as usize {
        return weighted_mean;
    }
    let first_active = dx
        .iter()
        .zip(weights.iter())
        .find(|(_, w)| **w > 0.0)
        .map(|(d, _)| *d)
        .unwrap_or(0.0);
    let all_same = dx
        .iter()
        .zip(weights.iter())
        .filter(|(_, w)| **w > 0.0)
        .all(|(d, _)| (*d - first_active).abs() < f64::EPSILON);
    if all_same {
        return weighted_mean;
    }

    // Construct the normal equations X^T W X β = X^T W y, where X is the
    // Vandermonde-like design matrix of centred abscissas.
    let p = degree as usize + 1;
    let mut xtwx = Array2::<f64>::zeros((p, p));
    let mut xtwy = Array1::<f64>::zeros(p);
    for ((d, y), w) in dx.iter().zip(ys.iter()).zip(weights.iter()) {
        if *w <= 0.0 {
            continue;
        }
        // Powers of (x - target) up to degree.
        let mut powers = Vec::with_capacity(p);
        let mut acc = 1.0_f64;
        for _ in 0..p {
            powers.push(acc);
            acc *= *d;
        }
        for r in 0..p {
            for c in 0..p {
                xtwx[[r, c]] += w * powers[r] * powers[c];
            }
            xtwy[r] += w * powers[r] * y;
        }
    }
    match solve_ndarray(&xtwx, &xtwy) {
        Ok(beta) => beta[0],
        Err(_) => weighted_mean,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tricube_basic() {
        assert!((tricube_weight(0.0, 1.0) - 1.0).abs() < 1e-12);
        assert!((tricube_weight(1.0, 1.0)).abs() < 1e-12);
        assert!(tricube_weight(2.0, 1.0) == 0.0);
        assert!(tricube_weight(0.5, 1.0) > 0.0);
    }

    #[test]
    fn weighted_polynomial_fit_local_constant() {
        let xs = vec![0.0, 1.0, 2.0, 3.0];
        let ys = vec![5.0; 4];
        let ws = vec![1.0; 4];
        let v = weighted_polynomial_fit_local(&xs, &ys, &ws, 1, 1.5);
        assert!((v - 5.0).abs() < 1e-12);
    }

    #[test]
    fn weighted_polynomial_fit_local_linear() {
        let xs = vec![0.0, 1.0, 2.0, 3.0];
        let ys = vec![1.0, 3.0, 5.0, 7.0];
        let ws = vec![1.0; 4];
        let v = weighted_polynomial_fit_local(&xs, &ys, &ws, 1, 1.5);
        assert!((v - 4.0).abs() < 1e-9);
    }
}

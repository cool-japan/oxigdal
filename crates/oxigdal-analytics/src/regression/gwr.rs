//! Geographically Weighted Regression (GWR)
//!
//! Geographically Weighted Regression (Brunsdon, Fotheringham & Charlton, 1996)
//! is a local form of linear regression that allows the relationship between a
//! response variable and one or more predictors to vary across geographic
//! space. Instead of estimating a single global coefficient vector, GWR fits a
//! separate weighted least squares (WLS) model at each observation location,
//! where nearby observations contribute more strongly than distant ones
//! through a distance-decay spatial kernel.
//!
//! At each location `s_i` the model solves the weighted normal equations
//!
//! ```text
//! β(s_i) = (Xᵀ W(s_i) X)⁻¹ Xᵀ W(s_i) y
//! ```
//!
//! where `W(s_i) = diag(w_ij)` and `w_ij = kernel(d_ij, b)` is the kernel
//! weight assigned to observation `j` when fitting at location `i`,
//! `d_ij` being the Euclidean distance between the two locations and `b` the
//! kernel bandwidth. The design matrix `X` includes an automatically added
//! intercept column of ones as its first column.
//!
//! # References
//!
//! - Brunsdon, C., Fotheringham, A. S., & Charlton, M. E. (1996).
//!   "Geographically Weighted Regression: A Method for Exploring Spatial
//!   Nonstationarity." *Geographical Analysis*, 28(4), 281-298.
//! - Fotheringham, A. S., Brunsdon, C., & Charlton, M. (2002).
//!   *Geographically Weighted Regression: The Analysis of Spatially Varying
//!   Relationships.* Wiley.

use crate::error::{AnalyticsError, Result};
use scirs2_core::ndarray::{Array1, Array2};

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Spatial kernel functions controlling how observation weights decay with
/// distance from the regression point.
///
/// In every kernel, `d` is the Euclidean distance between the regression point
/// and the observation, and `b` is the bandwidth (in distance units for a
/// fixed bandwidth, or the distance to the k-th nearest neighbour for an
/// adaptive bandwidth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GwrKernel {
    /// Gaussian kernel: `w = exp(-0.5 * (d / b)^2)`.
    ///
    /// Weights never reach exactly zero, so every observation contributes to
    /// every local fit (continuous, infinite support).
    Gaussian,
    /// Bisquare kernel: `w = (1 - (d / b)^2)^2` for `d < b`, otherwise `0`.
    ///
    /// Observations beyond the bandwidth receive zero weight (compact support).
    Bisquare,
    /// Exponential kernel: `w = exp(-d / b)`.
    ///
    /// Like the Gaussian kernel, weights are strictly positive everywhere but
    /// decay more slowly in the tails.
    Exponential,
}

impl GwrKernel {
    /// Evaluate the kernel weight for a distance `d` given bandwidth `b`.
    ///
    /// Returns `0.0` for non-positive or non-finite bandwidths, which makes the
    /// associated local system effectively rely only on the co-located point.
    #[must_use]
    pub fn weight(self, d: f64, b: f64) -> f64 {
        if !b.is_finite() || b <= 0.0 {
            // Degenerate bandwidth: only an exactly co-located point keeps a
            // non-zero weight; everything else collapses to zero.
            return if d == 0.0 { 1.0 } else { 0.0 };
        }
        match self {
            GwrKernel::Gaussian => {
                let r = d / b;
                (-0.5 * r * r).exp()
            }
            GwrKernel::Bisquare => {
                if d < b {
                    let r = d / b;
                    let t = 1.0 - r * r;
                    t * t
                } else {
                    0.0
                }
            }
            GwrKernel::Exponential => (-d / b).exp(),
        }
    }
}

/// Bandwidth specification controlling the spatial extent of each local fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GwrBandwidth {
    /// A single fixed bandwidth (in coordinate distance units) applied at every
    /// location.
    Fixed(f64),
    /// An adaptive bandwidth: at each location the bandwidth is the distance to
    /// its `k`-th nearest neighbour, so the kernel adapts to local point
    /// density.
    AdaptiveKnn(usize),
}

/// Configuration options for a GWR fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GwrOptions {
    /// Spatial kernel used to weight observations by distance.
    pub kernel: GwrKernel,
    /// Bandwidth specification (fixed distance or adaptive nearest-neighbour).
    pub bandwidth: GwrBandwidth,
    /// When `true`, the bandwidth is selected by minimising the corrected
    /// Akaike Information Criterion (AICc) via golden-section search. For a
    /// [`GwrBandwidth::Fixed`] bandwidth the search optimises the distance; for
    /// [`GwrBandwidth::AdaptiveKnn`] it optimises the neighbour count `k`.
    pub optimize_bandwidth: bool,
}

impl Default for GwrOptions {
    fn default() -> Self {
        Self {
            kernel: GwrKernel::Bisquare,
            bandwidth: GwrBandwidth::AdaptiveKnn(0),
            optimize_bandwidth: false,
        }
    }
}

/// Result of a geographically weighted regression fit.
#[derive(Debug, Clone)]
pub struct GwrResult {
    /// Per-location coefficient vectors. The outer `Vec` has one entry per
    /// observation; each inner `Vec` has length `n_predictors + 1`, with the
    /// intercept first followed by one coefficient per supplied predictor.
    pub coefficients: Vec<Vec<f64>>,
    /// Fitted (predicted) response value at each observation location.
    pub predicted: Vec<f64>,
    /// Residuals, `y_i - predicted_i`, at each observation location.
    pub residuals: Vec<f64>,
    /// Local coefficient of determination (R²) computed from the
    /// kernel-weighted observations used in each local fit.
    pub local_r2: Vec<f64>,
    /// Effective bandwidth used for the fit. For an adaptive bandwidth this is
    /// the neighbour count `k` (cast to `f64`); for a fixed bandwidth it is the
    /// distance.
    pub bandwidth: f64,
    /// Corrected Akaike Information Criterion (AICc) for the fitted model.
    pub aicc: f64,
}

/// A resolved bandwidth value used internally by the fitting routine.
///
/// This mirrors [`GwrBandwidth`] but stores the optimisation-friendly scalar
/// value (a distance for the fixed case or a neighbour count for the adaptive
/// case) so the golden-section search can operate on a single `f64` axis.
#[derive(Debug, Clone, Copy)]
enum ResolvedBandwidth {
    /// Fixed bandwidth distance.
    Fixed(f64),
    /// Adaptive nearest-neighbour bandwidth with `k` neighbours.
    AdaptiveKnn(usize),
}

impl ResolvedBandwidth {
    /// The scalar reported in [`GwrResult::bandwidth`].
    fn report_value(self) -> f64 {
        match self {
            ResolvedBandwidth::Fixed(b) => b,
            ResolvedBandwidth::AdaptiveKnn(k) => k as f64,
        }
    }
}

/// Outcome of fitting all local regressions for a single candidate bandwidth.
struct GwrFitArtifacts {
    coefficients: Vec<Vec<f64>>,
    predicted: Vec<f64>,
    residuals: Vec<f64>,
    local_r2: Vec<f64>,
    /// Trace of the hat matrix `S` (the effective number of parameters).
    trace_s: f64,
}

/// Fit a geographically weighted regression.
///
/// At each observation location a weighted least squares regression is solved
/// using all observations, weighted by a distance-decay [`GwrKernel`]. An
/// intercept column is added automatically, so the supplied `x` must contain
/// only the predictor values (without a column of ones).
///
/// # Arguments
///
/// * `coords` - Observation coordinates as `(x, y)` pairs, one per observation.
/// * `x` - Row-major predictor matrix: one inner `Vec` per observation holding
///   the predictor values for that observation, **without** the intercept.
/// * `y` - Response values, one per observation.
/// * `options` - Kernel, bandwidth, and optimisation configuration.
///
/// # Errors
///
/// Returns [`AnalyticsError`] if the inputs are inconsistent (mismatched
/// lengths, empty data, ragged predictor rows), if there are too few
/// observations to support the requested model, or if a local weighted normal
/// equation system is singular or ill-conditioned (rank-deficient).
pub fn gwr_fit(
    coords: &[(f64, f64)],
    x: &[Vec<f64>],
    y: &[f64],
    options: &GwrOptions,
) -> Result<GwrResult> {
    let n = coords.len();
    if n == 0 {
        return Err(AnalyticsError::insufficient_data(
            "GWR requires at least one observation",
        ));
    }
    if x.len() != n {
        return Err(AnalyticsError::dimension_mismatch(
            format!("{n} predictor rows"),
            format!("{} predictor rows", x.len()),
        ));
    }
    if y.len() != n {
        return Err(AnalyticsError::dimension_mismatch(
            format!("{n} response values"),
            format!("{} response values", y.len()),
        ));
    }

    // Number of predictors (excluding the intercept). All predictor rows must
    // share the same width.
    let n_predictors = x[0].len();
    for (i, row) in x.iter().enumerate() {
        if row.len() != n_predictors {
            return Err(AnalyticsError::dimension_mismatch(
                format!("predictor row 0 has {n_predictors} columns"),
                format!("predictor row {i} has {} columns", row.len()),
            ));
        }
    }

    // Total number of parameters, including the intercept.
    let n_params = n_predictors + 1;
    if n < n_params {
        return Err(AnalyticsError::insufficient_data(format!(
            "GWR with {n_predictors} predictor(s) needs at least {n_params} observations, got {n}"
        )));
    }
    for (name, values) in [("coordinates", None), ("response", Some(y))] {
        if let Some(values) = values {
            if values.iter().any(|v| !v.is_finite()) {
                return Err(AnalyticsError::invalid_input(format!(
                    "{name} values must be finite"
                )));
            }
        }
    }
    if coords
        .iter()
        .any(|(cx, cy)| !cx.is_finite() || !cy.is_finite())
    {
        return Err(AnalyticsError::invalid_input(
            "coordinate values must be finite",
        ));
    }
    if x.iter().any(|row| row.iter().any(|v| !v.is_finite())) {
        return Err(AnalyticsError::invalid_input(
            "predictor values must be finite",
        ));
    }

    // Build the augmented design matrix once (intercept + predictors).
    let design = build_design_matrix(x, n, n_params);

    // Precompute the full pairwise distance matrix; it is reused across every
    // local fit and, when optimising, across every candidate bandwidth.
    let distances = pairwise_distances(coords);

    // Resolve (and optionally optimise) the bandwidth.
    let resolved = if options.optimize_bandwidth {
        optimize_bandwidth(&design, y, &distances, options, n, n_params)?
    } else {
        resolve_initial_bandwidth(&distances, options, n)?
    };

    let artifacts = fit_all_locations(&design, y, &distances, options.kernel, resolved, n_params)?;
    let aicc = corrected_aic(n, &artifacts.residuals, artifacts.trace_s)?;

    Ok(GwrResult {
        coefficients: artifacts.coefficients,
        predicted: artifacts.predicted,
        residuals: artifacts.residuals,
        local_r2: artifacts.local_r2,
        bandwidth: resolved.report_value(),
        aicc,
    })
}

/// Build the `n x n_params` design matrix with an intercept column of ones
/// followed by the supplied predictor columns.
fn build_design_matrix(x: &[Vec<f64>], n: usize, n_params: usize) -> Array2<f64> {
    let mut design = Array2::<f64>::zeros((n, n_params));
    for i in 0..n {
        design[[i, 0]] = 1.0;
        for (j, value) in x[i].iter().enumerate() {
            design[[i, j + 1]] = *value;
        }
    }
    design
}

/// Compute the full symmetric matrix of pairwise Euclidean distances between
/// observation locations.
fn pairwise_distances(coords: &[(f64, f64)]) -> Array2<f64> {
    let n = coords.len();
    let mut distances = Array2::<f64>::zeros((n, n));
    for i in 0..n {
        let (xi, yi) = coords[i];
        for j in (i + 1)..n {
            let (xj, yj) = coords[j];
            let dx = xi - xj;
            let dy = yi - yj;
            let d = (dx * dx + dy * dy).sqrt();
            distances[[i, j]] = d;
            distances[[j, i]] = d;
        }
    }
    distances
}

/// Resolve the user-specified bandwidth into a concrete internal value,
/// validating it against the data.
fn resolve_initial_bandwidth(
    distances: &Array2<f64>,
    options: &GwrOptions,
    n: usize,
) -> Result<ResolvedBandwidth> {
    match options.bandwidth {
        GwrBandwidth::Fixed(b) => {
            if !b.is_finite() || b <= 0.0 {
                return Err(AnalyticsError::invalid_parameter(
                    "bandwidth",
                    "fixed bandwidth must be a positive, finite distance",
                ));
            }
            Ok(ResolvedBandwidth::Fixed(b))
        }
        GwrBandwidth::AdaptiveKnn(k) => {
            // A zero or unset neighbour count defaults to a sensible value that
            // guarantees enough points for a stable local fit; otherwise clamp
            // to the available sample size.
            let default_k = ((n as f64).sqrt().ceil() as usize).max(2);
            let chosen = if k == 0 { default_k } else { k };
            let clamped = chosen.min(n.saturating_sub(1)).max(1);
            let _ = distances;
            Ok(ResolvedBandwidth::AdaptiveKnn(clamped))
        }
    }
}

/// Determine the local bandwidth distance for observation `i`.
///
/// For a fixed bandwidth this is constant; for an adaptive bandwidth it is the
/// distance from `i` to its `k`-th nearest neighbour (excluding itself).
fn local_bandwidth_distance(
    distances: &Array2<f64>,
    resolved: ResolvedBandwidth,
    i: usize,
    n: usize,
) -> f64 {
    match resolved {
        ResolvedBandwidth::Fixed(b) => b,
        ResolvedBandwidth::AdaptiveKnn(k) => {
            // Collect distances from i to every other point, sort ascending and
            // take the k-th. `k` is already clamped to [1, n-1].
            let mut row: Vec<f64> = (0..n)
                .filter(|&j| j != i)
                .map(|j| distances[[i, j]])
                .collect();
            row.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let idx = (k - 1).min(row.len().saturating_sub(1));
            let b = row.get(idx).copied().unwrap_or(0.0);
            // Guard against coincident points giving a zero bandwidth: fall
            // back to the largest available neighbour distance so the kernel
            // remains well defined.
            if b > 0.0 {
                b
            } else {
                row.iter()
                    .copied()
                    .fold(0.0_f64, f64::max)
                    .max(f64::EPSILON)
            }
        }
    }
}

/// Fit the local weighted regression at every observation location and collect
/// the per-location coefficients, predictions, residuals, local R², and the
/// trace of the hat matrix.
fn fit_all_locations(
    design: &Array2<f64>,
    y: &[f64],
    distances: &Array2<f64>,
    kernel: GwrKernel,
    resolved: ResolvedBandwidth,
    n_params: usize,
) -> Result<GwrFitArtifacts> {
    let n = y.len();

    // Each closure produces (coefficients, predicted, residual, local_r2,
    // leverage) for a single location, or an error if the local system is
    // singular.
    let solve_one = |i: usize| -> Result<(Vec<f64>, f64, f64, f64, f64)> {
        let b = local_bandwidth_distance(distances, resolved, i, n);
        solve_local(design, y, distances, kernel, i, b, n_params)
    };

    #[cfg(feature = "parallel")]
    let per_location: Vec<(Vec<f64>, f64, f64, f64, f64)> = (0..n)
        .into_par_iter()
        .map(solve_one)
        .collect::<Result<Vec<_>>>()?;

    #[cfg(not(feature = "parallel"))]
    let per_location: Vec<(Vec<f64>, f64, f64, f64, f64)> =
        (0..n).map(solve_one).collect::<Result<Vec<_>>>()?;

    let mut coefficients = Vec::with_capacity(n);
    let mut predicted = Vec::with_capacity(n);
    let mut residuals = Vec::with_capacity(n);
    let mut local_r2 = Vec::with_capacity(n);
    let mut trace_s = 0.0;
    for (beta, pred, resid, r2, leverage) in per_location {
        coefficients.push(beta);
        predicted.push(pred);
        residuals.push(resid);
        local_r2.push(r2);
        trace_s += leverage;
    }

    Ok(GwrFitArtifacts {
        coefficients,
        predicted,
        residuals,
        local_r2,
        trace_s,
    })
}

/// Solve the weighted least squares system at a single location `i`.
///
/// Returns `(coefficients, predicted_i, residual_i, local_r2, leverage_ii)`
/// where `leverage_ii` is the `i`-th diagonal element of the hat matrix used
/// to accumulate the trace of `S`.
fn solve_local(
    design: &Array2<f64>,
    y: &[f64],
    distances: &Array2<f64>,
    kernel: GwrKernel,
    i: usize,
    bandwidth: f64,
    n_params: usize,
) -> Result<(Vec<f64>, f64, f64, f64, f64)> {
    let n = y.len();

    // Kernel weights for every observation relative to location i.
    let mut weights = vec![0.0_f64; n];
    let mut weight_sum = 0.0;
    for (j, w) in weights.iter_mut().enumerate() {
        let d = distances[[i, j]];
        let kw = kernel.weight(d, bandwidth);
        *w = kw;
        weight_sum += kw;
    }
    if weight_sum <= 0.0 || !weight_sum.is_finite() {
        return Err(AnalyticsError::numerical_instability(format!(
            "local kernel weights at location {i} sum to a non-positive value; bandwidth too small"
        )));
    }

    // Form the weighted normal equations: A = Xᵀ W X, rhs = Xᵀ W y.
    let mut a = Array2::<f64>::zeros((n_params, n_params));
    let mut rhs = Array1::<f64>::zeros(n_params);
    for j in 0..n {
        let w = weights[j];
        if w == 0.0 {
            continue;
        }
        for p in 0..n_params {
            let xjp = design[[j, p]];
            let wx = w * xjp;
            rhs[p] += wx * y[j];
            for q in p..n_params {
                a[[p, q]] += wx * design[[j, q]];
            }
        }
    }
    // Mirror the upper triangle into the lower triangle (A is symmetric).
    for p in 0..n_params {
        for q in (p + 1)..n_params {
            a[[q, p]] = a[[p, q]];
        }
    }

    // Invert A using the same Gauss-Jordan elimination strategy used by the
    // kriging module, guarding against singular (rank-deficient) systems.
    let a_inv = invert_matrix(&a)?;

    // Coefficients β = A⁻¹ rhs.
    let mut beta = vec![0.0_f64; n_params];
    for p in 0..n_params {
        let mut acc = 0.0;
        for q in 0..n_params {
            acc += a_inv[[p, q]] * rhs[q];
        }
        beta[p] = acc;
    }
    if beta.iter().any(|c| !c.is_finite()) {
        return Err(AnalyticsError::numerical_instability(format!(
            "local coefficients at location {i} are not finite; system is ill-conditioned"
        )));
    }

    // Prediction at location i using its own predictor row.
    let mut predicted = 0.0;
    for p in 0..n_params {
        predicted += beta[p] * design[[i, p]];
    }
    let residual = y[i] - predicted;

    // Leverage h_ii = x_iᵀ (Xᵀ W X)⁻¹ x_i w_ii — the i-th diagonal of the hat
    // matrix S whose trace gives the effective number of parameters.
    let leverage = hat_diagonal(design, &a_inv, weights[i], i, n_params);

    // Local (weighted) R² from the kernel-weighted observations.
    let local_r2 = local_r_squared(design, y, &weights, &beta, weight_sum, n_params);

    Ok((beta, predicted, residual, local_r2, leverage))
}

/// Compute the `i`-th diagonal element of the hat matrix for the local fit:
/// `h_ii = w_ii * x_iᵀ (Xᵀ W X)⁻¹ x_i`.
fn hat_diagonal(
    design: &Array2<f64>,
    a_inv: &Array2<f64>,
    w_ii: f64,
    i: usize,
    n_params: usize,
) -> f64 {
    // Compute t = (Xᵀ W X)⁻¹ x_i, then x_iᵀ t, scaled by the self weight.
    let mut quad = 0.0;
    for p in 0..n_params {
        let mut t_p = 0.0;
        for q in 0..n_params {
            t_p += a_inv[[p, q]] * design[[i, q]];
        }
        quad += design[[i, p]] * t_p;
    }
    w_ii * quad
}

/// Compute the kernel-weighted local coefficient of determination (R²) for the
/// fit centred at a location, using the supplied kernel weights.
fn local_r_squared(
    design: &Array2<f64>,
    y: &[f64],
    weights: &[f64],
    beta: &[f64],
    weight_sum: f64,
    n_params: usize,
) -> f64 {
    let n = y.len();

    // Weighted mean of the response.
    let mut weighted_y = 0.0;
    for j in 0..n {
        weighted_y += weights[j] * y[j];
    }
    let mean_y = weighted_y / weight_sum;

    // Weighted total and residual sums of squares.
    let mut ss_tot = 0.0;
    let mut ss_res = 0.0;
    for j in 0..n {
        let w = weights[j];
        if w == 0.0 {
            continue;
        }
        let mut fitted = 0.0;
        for p in 0..n_params {
            fitted += beta[p] * design[[j, p]];
        }
        let dy = y[j] - mean_y;
        let dr = y[j] - fitted;
        ss_tot += w * dy * dy;
        ss_res += w * dr * dr;
    }

    if ss_tot <= f64::EPSILON {
        // No weighted variance in the response (e.g. constant data): a perfect
        // fit explains all (zero) variance.
        1.0
    } else {
        let r2 = 1.0 - ss_res / ss_tot;
        r2.clamp(0.0, 1.0)
    }
}

/// Compute the corrected Akaike Information Criterion (AICc) for the fit:
/// `AICc = 2n·ln(σ̂) + n·ln(2π) + n·(n + tr(S)) / (n − 2 − tr(S))`, where `σ̂`
/// is the RSS-based residual standard deviation.
fn corrected_aic(n: usize, residuals: &[f64], trace_s: f64) -> Result<f64> {
    let nf = n as f64;
    let rss: f64 = residuals.iter().map(|r| r * r).sum();

    // RSS-based residual standard deviation (maximum-likelihood form).
    let sigma2 = rss / nf;
    let sigma = sigma2.sqrt();
    if !sigma.is_finite() {
        return Err(AnalyticsError::numerical_instability(
            "residual standard deviation is not finite while computing AICc",
        ));
    }
    // Guard a perfect fit (RSS == 0): AICc tends to -inf, so report a large
    // negative-but-finite penalty floor for sigma to keep the criterion usable
    // for comparison during bandwidth search.
    let safe_sigma = if sigma <= 0.0 {
        f64::MIN_POSITIVE
    } else {
        sigma
    };

    let denom = nf - 2.0 - trace_s;
    if denom.abs() <= f64::EPSILON {
        // Effective degrees of freedom exhausted; treat as an infinitely poor
        // model so the optimiser avoids this bandwidth.
        return Ok(f64::INFINITY);
    }

    let aicc = 2.0 * nf * safe_sigma.ln()
        + nf * (2.0 * std::f64::consts::PI).ln()
        + nf * (nf + trace_s) / denom;
    Ok(aicc)
}

/// Select a bandwidth by minimising the corrected AIC via golden-section
/// search over a sensible range derived from the pairwise distances.
fn optimize_bandwidth(
    design: &Array2<f64>,
    y: &[f64],
    distances: &Array2<f64>,
    options: &GwrOptions,
    n: usize,
    n_params: usize,
) -> Result<ResolvedBandwidth> {
    match options.bandwidth {
        GwrBandwidth::Fixed(_) => {
            let (lo, hi) = fixed_bandwidth_bounds(distances, n)?;
            let score = |b: f64| -> f64 {
                evaluate_aicc(
                    design,
                    y,
                    distances,
                    options.kernel,
                    ResolvedBandwidth::Fixed(b),
                    n_params,
                )
            };
            let best = golden_section_min(lo, hi, &score);
            Ok(ResolvedBandwidth::Fixed(best))
        }
        GwrBandwidth::AdaptiveKnn(_) => {
            // Search over integer neighbour counts in [k_min, k_max].
            let k_min = n_params.max(2);
            let k_max = n.saturating_sub(1).max(k_min);
            let score = |k: f64| -> f64 {
                let k_round = (k.round() as usize).clamp(k_min, k_max);
                evaluate_aicc(
                    design,
                    y,
                    distances,
                    options.kernel,
                    ResolvedBandwidth::AdaptiveKnn(k_round),
                    n_params,
                )
            };
            let best = golden_section_min(k_min as f64, k_max as f64, &score);
            let best_k = (best.round() as usize).clamp(k_min, k_max);
            Ok(ResolvedBandwidth::AdaptiveKnn(best_k))
        }
    }
}

/// Evaluate the AICc for a candidate bandwidth, returning `f64::INFINITY` if any
/// local fit fails (so the optimiser steers away from infeasible bandwidths).
fn evaluate_aicc(
    design: &Array2<f64>,
    y: &[f64],
    distances: &Array2<f64>,
    kernel: GwrKernel,
    resolved: ResolvedBandwidth,
    n_params: usize,
) -> f64 {
    match fit_all_locations(design, y, distances, kernel, resolved, n_params) {
        Ok(artifacts) => match corrected_aic(y.len(), &artifacts.residuals, artifacts.trace_s) {
            Ok(value) if value.is_finite() => value,
            _ => f64::INFINITY,
        },
        Err(_) => f64::INFINITY,
    }
}

/// Derive a `[min, max]` fixed-bandwidth search range from the pairwise
/// distances: the lower bound is a small fraction of the maximum pairwise
/// distance (large enough to keep local samples non-degenerate), the upper
/// bound is the maximum pairwise distance.
fn fixed_bandwidth_bounds(distances: &Array2<f64>, n: usize) -> Result<(f64, f64)> {
    let mut max_d = 0.0_f64;
    for i in 0..n {
        for j in (i + 1)..n {
            max_d = max_d.max(distances[[i, j]]);
        }
    }
    if max_d <= 0.0 || !max_d.is_finite() {
        return Err(AnalyticsError::insufficient_data(
            "all observation coordinates are coincident; cannot derive a bandwidth range",
        ));
    }
    let lo = (max_d / (n as f64)).max(max_d * 1e-3);
    let hi = max_d;
    Ok((lo.min(hi), hi))
}

/// Minimise a unimodal (or near-unimodal) scalar function on `[a, b]` using
/// golden-section search. Returns the argument that yields the smallest value
/// among the interior evaluations.
fn golden_section_min<F>(mut a: f64, mut b: f64, f: &F) -> f64
where
    F: Fn(f64) -> f64,
{
    if a >= b {
        return a;
    }
    // Reciprocal golden ratio.
    let inv_phi = (5.0_f64.sqrt() - 1.0) / 2.0;
    let inv_phi2 = inv_phi * inv_phi;

    let mut h = b - a;
    let mut c = a + inv_phi2 * h;
    let mut d = a + inv_phi * h;
    let mut fc = f(c);
    let mut fd = f(d);

    // Best-so-far across all sampled points (robust if the AICc surface is not
    // strictly unimodal).
    let mut best_x = if fc <= fd { c } else { d };
    let mut best_f = fc.min(fd);

    // Fixed iteration count gives a tight bracket without an explicit tolerance.
    for _ in 0..100 {
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            h = b - a;
            c = a + inv_phi2 * h;
            fc = f(c);
            if fc < best_f {
                best_f = fc;
                best_x = c;
            }
        } else {
            a = c;
            c = d;
            fc = fd;
            h = b - a;
            d = a + inv_phi * h;
            fd = f(d);
            if fd < best_f {
                best_f = fd;
                best_x = d;
            }
        }
        if h.abs() <= f64::EPSILON {
            break;
        }
    }
    best_x
}

/// Invert a square matrix using Gauss-Jordan elimination with partial pivoting.
///
/// This mirrors the inversion strategy used by the kriging module so the GWR
/// solver shares the same numerical linear-algebra path. A singular or
/// ill-conditioned system (zero pivot) yields a [`AnalyticsError::MatrixError`]
/// rather than a panic, which the caller surfaces as a rank-deficiency error.
fn invert_matrix(matrix: &Array2<f64>) -> Result<Array2<f64>> {
    let n = matrix.nrows();
    if n != matrix.ncols() {
        return Err(AnalyticsError::matrix_error(
            "local normal-equation matrix must be square",
        ));
    }

    // Augmented matrix [A | I].
    let mut aug = Array2::<f64>::zeros((n, 2 * n));
    for i in 0..n {
        for j in 0..n {
            aug[[i, j]] = matrix[[i, j]];
        }
        aug[[i, n + i]] = 1.0;
    }

    for i in 0..n {
        // Partial pivoting: find the largest-magnitude pivot in the column.
        let mut max_row = i;
        let mut max_val = aug[[i, i]].abs();
        for k in (i + 1)..n {
            let v = aug[[k, i]].abs();
            if v > max_val {
                max_val = v;
                max_row = k;
            }
        }

        if max_val < 1e-12 {
            return Err(AnalyticsError::matrix_error(
                "local weighted regression system is singular or rank-deficient",
            ));
        }

        if max_row != i {
            for j in 0..(2 * n) {
                let tmp = aug[[i, j]];
                aug[[i, j]] = aug[[max_row, j]];
                aug[[max_row, j]] = tmp;
            }
        }

        let pivot = aug[[i, i]];
        for j in 0..(2 * n) {
            aug[[i, j]] /= pivot;
        }

        for k in 0..n {
            if k != i {
                let factor = aug[[k, i]];
                if factor != 0.0 {
                    for j in 0..(2 * n) {
                        aug[[k, j]] -= factor * aug[[i, j]];
                    }
                }
            }
        }
    }

    let mut inverse = Array2::<f64>::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            inverse[[i, j]] = aug[[i, n + j]];
        }
    }

    if inverse.iter().any(|v| !v.is_finite()) {
        return Err(AnalyticsError::numerical_instability(
            "matrix inverse produced non-finite entries (ill-conditioned system)",
        ));
    }

    Ok(inverse)
}

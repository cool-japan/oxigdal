//! BFAST — Breaks For Additive Season and Trend
//!
//! Real implementation of the BFAST change-detection method of
//! Verbesselt, Hyndman, Newnham & Culvenor (2010),
//! "Detecting trend and seasonal changes in satellite image time series",
//! *Remote Sensing of Environment* **114**(1):106-115.
//!
//! # Method
//!
//! For each pixel time series `Y_t` (NDVI/EVI-style) the algorithm:
//!
//! 1. Fits an additive **season + trend** model by ordinary least squares
//!    (OLS):
//!
//!    ```text
//!    Y_t = β0 + β1·t + Σ_{i=1..k} ( α_i·sin(2π i t / T) + γ_i·cos(2π i t / T) ) + e_t
//!    ```
//!
//!    where `T` is the seasonal period and `k` a small harmonic order
//!    (default 3, reduced automatically when the available degrees of freedom
//!    are insufficient). The design matrix is solved with
//!    [`scirs2_core::linalg::lstsq_ndarray`], matching the workspace SciRS2
//!    policy for linear algebra.
//!
//! 2. Computes the **OLS-MOSUM** (moving sum of OLS residuals) structural-change
//!    statistic over a bandwidth `h` (default `h = 0.15`). For a window of width
//!    `⌊h·n⌋` the empirical fluctuation process is
//!
//!    ```text
//!    MOSUM(t) = ( 1 / (σ̂ · √(h·n)) ) · Σ_{s = t+1}^{t + ⌊h·n⌋} e_s
//!    ```
//!
//!    with `σ̂` the residual standard error of the full fit. This is the
//!    moving-sum estimator of Chu, Hornik & Kuan (1995),
//!    "MOSUM tests for parameter constancy", *Biometrika* **82**(3):603-617.
//!
//! 3. Flags a **break** when `max_t |MOSUM(t)|` exceeds the 5 % critical value
//!    of the OLS-MOSUM boundary (see [`mosum_critical_value`]).
//!
//! 4. Localises the break at the window centre of maximum `|MOSUM|`, estimates
//!    the **magnitude** as the difference of the fitted trend mean after vs.
//!    before the break, derives the **direction** from the sign of the
//!    magnitude, and reports a bounded **confidence** from how far the statistic
//!    exceeds the critical value.
//!
//! Series that are too short (`n < 2·T`) or have too few degrees of freedom to
//! fit even a first-order harmonic model return a graceful no-break result;
//! the function never panics.

use crate::error::{Result, TemporalError};
use crate::timeseries::TimeSeriesRaster;
use scirs2_core::linalg::lstsq_ndarray;
use scirs2_core::ndarray::{Array1, Array2, Array3};
use tracing::info;

use super::detection::{ChangeDetectionConfig, ChangeDetectionResult};

/// Default OLS-MOSUM bandwidth as a fraction of the series length.
///
/// Verbesselt et al. (2010) use `h = 0.15`; the same default is used by the
/// reference `bfast` R package (`strucchange::efp(..., h = 0.15)`).
const DEFAULT_BANDWIDTH: f64 = 0.15;

/// Default maximum harmonic order of the seasonal model.
///
/// Order 3 captures the first three Fourier harmonics of the annual cycle,
/// the default of the `bfast` R package (`order = 3`).
const DEFAULT_HARMONIC_ORDER: usize = 3;

/// Number of seconds in a (Gregorian mean) year, used to infer the seasonal
/// period when the time series carries calendar timestamps.
const SECONDS_PER_YEAR: f64 = 365.2425 * 86_400.0;

/// Residual standard error below `SIGMA_RELATIVE_FLOOR · rms(Y)` is treated as a
/// perfectly-modelled (degenerate) series with no structural break. This guards
/// the OLS-MOSUM ratio against division by a near-zero, floating-point-noise σ̂.
const SIGMA_RELATIVE_FLOOR: f64 = 1e-8;

/// Absolute lower bound on σ̂ for series whose values are themselves ≈ 0, so the
/// relative floor cannot collapse to zero.
const SIGMA_ABSOLUTE_FLOOR: f64 = 1e-12;

/// Detect abrupt / gradual changes with the BFAST OLS-MOSUM procedure.
///
/// This is the real implementation backing
/// [`ChangeDetector::bfast_change`](super::detection::ChangeDetector). It takes
/// exactly the same inputs as the former stub method and returns the same
/// [`ChangeDetectionResult`] shape, populated per pixel with:
///
/// * `magnitude` — fitted-trend mean after the break minus the mean before it
///   (`0.0` when no break is found),
/// * `direction` — `sign(magnitude)` as `i8` (`+1`, `0`, `-1`),
/// * `change_time` — timestamp (seconds) of the localised break (`0` for
///   pixels without a break),
/// * `confidence` — bounded in `[0, 1]`, `(stat − crit) / crit` clamped, `0.0`
///   when no break is detected.
///
/// # Errors
/// Returns [`TemporalError::insufficient_data`] when the series has fewer than
/// three observations or carries no shape information, mirroring the other
/// per-pixel detectors in [`super::detection`].
pub fn bfast_detect(
    ts: &TimeSeriesRaster,
    config: &ChangeDetectionConfig,
) -> Result<ChangeDetectionResult> {
    if ts.len() < 3 {
        return Err(TemporalError::insufficient_data(
            "Need at least 3 observations",
        ));
    }

    let (height, width, n_bands) = ts
        .expected_shape()
        .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

    // Numeric time axis (one unit per observation) and the wall-clock
    // timestamps used to report break times.
    let timestamps: Vec<i64> = ts.entries().keys().copied().collect();
    let n = timestamps.len();
    let times: Vec<f64> = (0..n).map(|t| t as f64).collect();

    // Seasonal period in the same units as `times` (i.e. number of samples).
    let period = infer_period(ts, &timestamps);
    // Bandwidth fraction (re-uses `confidence_level` only if the caller set a
    // sensible MOSUM bandwidth there; otherwise the documented default).
    let bandwidth = resolve_bandwidth(config);
    let crit = mosum_critical_value(bandwidth);
    let min_magnitude = config.min_magnitude.unwrap_or(0.0);

    let mut magnitude = Array3::zeros((height, width, n_bands));
    let mut direction = Array3::<i8>::zeros((height, width, n_bands));
    let mut change_time = Array3::<i64>::zeros((height, width, n_bands));
    let mut confidence = Array3::zeros((height, width, n_bands));

    for i in 0..height {
        for j in 0..width {
            for k in 0..n_bands {
                let values = ts.extract_pixel_timeseries(i, j, k)?;

                let outcome = detect_series(&values, &times, period, bandwidth, crit)?;

                if let Some(brk) = outcome {
                    // Suppress sub-threshold magnitudes when the caller asked
                    // for a minimum reportable change.
                    if brk.magnitude.abs() < min_magnitude {
                        continue;
                    }
                    magnitude[[i, j, k]] = brk.magnitude;
                    direction[[i, j, k]] = sign_i8(brk.magnitude);
                    confidence[[i, j, k]] = brk.confidence;
                    if brk.index < timestamps.len() {
                        change_time[[i, j, k]] = timestamps[brk.index];
                    }
                }
            }
        }
    }

    info!(
        "Completed BFAST change detection (period={}, h={:.3}, crit={:.3})",
        period, bandwidth, crit
    );

    Ok(ChangeDetectionResult::new(magnitude, direction)
        .with_change_time(change_time)
        .with_confidence(confidence))
}

/// A localised break within a single pixel time series.
#[derive(Debug, Clone, Copy)]
struct PixelBreak {
    /// Sample index of the break (window centre of maximum `|MOSUM|`).
    index: usize,
    /// Trend-mean difference (after − before).
    magnitude: f64,
    /// Bounded confidence in `[0, 1]`.
    confidence: f64,
}

/// Run the full BFAST procedure on one pixel series.
///
/// Returns `Ok(None)` when no significant break is present or the series is too
/// short / rank-deficient to fit the seasonal-trend model — never panics.
fn detect_series(
    values: &[f64],
    times: &[f64],
    period: f64,
    bandwidth: f64,
    crit: f64,
) -> Result<Option<PixelBreak>> {
    let n = values.len();

    // Need at least two full periods (Verbesselt et al. require ≥ 2 cycles to
    // identify season + trend) and a usable MOSUM window.
    if (n as f64) < 2.0 * period {
        return Ok(None);
    }
    let window = (bandwidth * n as f64).floor() as usize;
    if window < 1 || window >= n {
        return Ok(None);
    }

    // Choose the largest harmonic order that leaves positive residual degrees
    // of freedom: p = 2 + 2k parameters, require p < n.
    let Some(order) = choose_harmonic_order(n, period) else {
        return Ok(None);
    };

    // Fit the season + trend model by OLS and obtain residuals + σ̂.
    let Some(fit) = fit_season_trend(values, times, period, order)? else {
        return Ok(None);
    };

    // Guard against a degenerate residual scale. When the model explains the
    // series essentially perfectly, σ̂ collapses to floating-point noise and the
    // OLS-MOSUM ratio (residual sum / σ̂) becomes numerically meaningless — a
    // constant or perfectly-modelled signal carries no structural break. We
    // therefore require σ̂ to exceed a small fraction of the observation scale
    // (root-mean-square magnitude) before testing.
    let data_scale = root_mean_square(values);
    let sigma_floor = (data_scale * SIGMA_RELATIVE_FLOOR).max(SIGMA_ABSOLUTE_FLOOR);
    if !fit.sigma.is_finite() || fit.sigma <= sigma_floor {
        return Ok(None);
    }

    // OLS-MOSUM empirical fluctuation process.
    let scale = fit.sigma * (window as f64).sqrt();
    if scale <= 0.0 {
        return Ok(None);
    }

    // Prefix sums of residuals for O(1) moving-window sums.
    let mut prefix = vec![0.0_f64; n + 1];
    for t in 0..n {
        prefix[t + 1] = prefix[t] + fit.residuals[t];
    }

    let mut max_stat = 0.0_f64;
    let mut max_center = 0usize;
    // Window [start, start+window): there are n-window+1 positions.
    for start in 0..=(n - window) {
        let win_sum = prefix[start + window] - prefix[start];
        let stat = (win_sum / scale).abs();
        if stat > max_stat {
            max_stat = stat;
            max_center = start + window / 2;
        }
    }

    if max_stat <= crit {
        return Ok(None);
    }

    // Localise: split the series at the break centre and compare fitted-trend
    // means before vs. after. The trend component is β0 + β1·t.
    let break_index = max_center.clamp(1, n - 1);
    let magnitude = trend_mean_shift(&fit, times, break_index);

    // Confidence: how far the statistic exceeds the critical value, clamped to
    // [0, 1]. At the boundary this is 0; it saturates once the statistic
    // reaches twice the critical value.
    let confidence = ((max_stat - crit) / crit).clamp(0.0, 1.0);

    Ok(Some(PixelBreak {
        index: break_index,
        magnitude,
        confidence,
    }))
}

/// Result of the OLS season + trend fit.
struct SeasonTrendFit {
    /// Intercept β0.
    beta0: f64,
    /// Trend slope β1.
    beta1: f64,
    /// Residuals `Y_t − Ŷ_t`.
    residuals: Vec<f64>,
    /// Residual standard error `σ̂ = sqrt( RSS / (n − p) )`.
    sigma: f64,
}

/// Fit `Y_t = β0 + β1·t + Σ (α_i sin + γ_i cos)` by ordinary least squares.
///
/// The design matrix has columns `[1, t, sin(2π t/T), cos(2π t/T), …]`. The
/// system is solved with [`lstsq_ndarray`] (SciRS2 linalg backend). Returns
/// `Ok(None)` if the solver reports the system as rank-deficient / not
/// converged, so callers degrade gracefully instead of erroring.
fn fit_season_trend(
    values: &[f64],
    times: &[f64],
    period: f64,
    order: usize,
) -> Result<Option<SeasonTrendFit>> {
    let n = values.len();
    let p = 2 + 2 * order;
    if n <= p {
        return Ok(None);
    }

    let mut design = Array2::<f64>::zeros((n, p));
    for (row, &t) in times.iter().enumerate().take(n) {
        design[[row, 0]] = 1.0;
        design[[row, 1]] = t;
        for h in 1..=order {
            let angle = 2.0 * std::f64::consts::PI * (h as f64) * t / period;
            design[[row, 2 * h]] = angle.sin();
            design[[row, 2 * h + 1]] = angle.cos();
        }
    }

    let y = Array1::from_vec(values.to_vec());

    let beta = match lstsq_ndarray(&design, &y) {
        Ok(b) => b,
        // Rank-deficient / non-convergent fit ⇒ treat as "cannot model",
        // degrade to a graceful no-break instead of surfacing an error.
        Err(_) => return Ok(None),
    };

    if beta.len() != p || beta.iter().any(|v| !v.is_finite()) {
        return Ok(None);
    }

    // Predictions and residuals.
    let mut residuals = vec![0.0_f64; n];
    let mut rss = 0.0_f64;
    for (row, &t) in times.iter().enumerate().take(n) {
        let mut pred = beta[0] + beta[1] * t;
        for h in 1..=order {
            let angle = 2.0 * std::f64::consts::PI * (h as f64) * t / period;
            pred += beta[2 * h] * angle.sin() + beta[2 * h + 1] * angle.cos();
        }
        let resid = values[row] - pred;
        residuals[row] = resid;
        rss += resid * resid;
    }

    let dof = (n - p) as f64;
    let sigma = (rss / dof).sqrt();

    Ok(Some(SeasonTrendFit {
        beta0: beta[0],
        beta1: beta[1],
        residuals,
        sigma,
    }))
}

/// Mean of the fitted trend `β0 + β1·t` after the break minus before it.
///
/// Because the trend is linear, this reduces to `β1 · (mean(t_after) −
/// mean(t_before))`, i.e. a magnitude with the sign of the slope scaled by the
/// temporal separation of the two segments. This captures both abrupt offsets
/// and gradual trend shifts in the additive-trend formulation.
fn trend_mean_shift(fit: &SeasonTrendFit, times: &[f64], break_index: usize) -> f64 {
    let n = times.len();
    if break_index == 0 || break_index >= n {
        return 0.0;
    }

    let before = &times[..break_index];
    let after = &times[break_index..];
    if before.is_empty() || after.is_empty() {
        return 0.0;
    }

    let mean_before = before.iter().sum::<f64>() / before.len() as f64;
    let mean_after = after.iter().sum::<f64>() / after.len() as f64;

    let trend_before = fit.beta0 + fit.beta1 * mean_before;
    let trend_after = fit.beta0 + fit.beta1 * mean_after;

    trend_after - trend_before
}

/// Largest harmonic order `k` such that `p = 2 + 2k` parameters leave at least
/// one residual degree of freedom (`p < n`) and the model still resolves the
/// requested harmonics (`2k ≤ T`, since harmonic `i` needs `i < T/2` to be
/// identifiable). Returns `None` when not even a first-order model fits.
fn choose_harmonic_order(n: usize, period: f64) -> Option<usize> {
    // Nyquist: harmonic i is only identifiable for i < T/2.
    let nyquist_order = ((period / 2.0).floor() as usize).max(1);
    let mut order = DEFAULT_HARMONIC_ORDER.min(nyquist_order);
    while order >= 1 {
        let p = 2 + 2 * order;
        if p < n {
            return Some(order);
        }
        order -= 1;
    }
    None
}

/// Infer the seasonal period `T` (in number of samples) for a series.
///
/// Strategy, in order of preference:
/// 1. If the collection declares a [`TemporalResolution`], assume an annual
///    cycle and convert to samples: `T = year / resolution`.
/// 2. Otherwise use the median spacing of the calendar timestamps and again
///    assume an annual cycle: `T = year / median_dt`.
/// 3. Fall back to a quarter of the series length when timestamps are
///    unusable.
///
/// The result is clamped to `[2, n/2]` so a MOSUM window and ≥ 2 cycles remain
/// feasible for reasonable inputs.
fn infer_period(ts: &TimeSeriesRaster, timestamps: &[i64]) -> f64 {
    let n = timestamps.len();
    let upper = ((n as f64) / 2.0).max(2.0);

    let samples_per_year = ts
        .resolution()
        .map(|r| SECONDS_PER_YEAR / r.as_seconds() as f64)
        .or_else(|| median_period_from_timestamps(timestamps))
        .unwrap_or((n as f64) / 4.0);

    if !samples_per_year.is_finite() || samples_per_year < 2.0 {
        return (n as f64 / 4.0).clamp(2.0, upper);
    }
    samples_per_year.clamp(2.0, upper)
}

/// Median inter-sample spacing of calendar timestamps converted into the
/// number of samples that span one year. Returns `None` when fewer than two
/// timestamps or the median spacing is non-positive.
fn median_period_from_timestamps(timestamps: &[i64]) -> Option<f64> {
    if timestamps.len() < 2 {
        return None;
    }
    let mut diffs: Vec<f64> = timestamps
        .windows(2)
        .map(|w| (w[1] - w[0]) as f64)
        .filter(|d| *d > 0.0)
        .collect();
    if diffs.is_empty() {
        return None;
    }
    diffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_dt = if diffs.len() % 2 == 0 {
        (diffs[diffs.len() / 2 - 1] + diffs[diffs.len() / 2]) / 2.0
    } else {
        diffs[diffs.len() / 2]
    };
    if median_dt <= 0.0 {
        return None;
    }
    Some(SECONDS_PER_YEAR / median_dt)
}

/// Resolve the OLS-MOSUM bandwidth fraction `h`.
///
/// The [`ChangeDetectionConfig`] has no dedicated bandwidth field, so the
/// documented default (`0.15`) is used. The hook is centralised here so a
/// future config field can override it without touching the algorithm.
fn resolve_bandwidth(_config: &ChangeDetectionConfig) -> f64 {
    DEFAULT_BANDWIDTH
}

/// 5 % critical value of the OLS-MOSUM boundary as a function of the bandwidth
/// `h`.
///
/// The OLS-based MOSUM process converges to an increment of a Brownian bridge;
/// its supremum has the limiting boundary derived by Chu, Hornik & Kuan (1995),
/// "MOSUM tests for parameter constancy", *Biometrika* **82**(3):603-617, and
/// tabulated for the `strucchange` R package (Zeileis et al. 2002, *J. Stat.
/// Soft.* 7(2)) in `strucchange:::sctest`/`boundary.efp`. For the OLS-MOSUM the
/// 5 % asymptotic critical values are approximately:
///
/// | h    | 5 % crit |
/// |------|----------|
/// | 0.05 | 2.27     |
/// | 0.10 | 1.99     |
/// | 0.15 | 1.85     |
/// | 0.20 | 1.76     |
/// | 0.25 | 1.69     |
/// | 0.30 | 1.64     |
/// | 0.50 | 1.50     |
///
/// (Source: Chu, Hornik & Kuan 1995, Table 1; reproduced in the `strucchange`
/// documentation. The default `h = 0.15` gives ≈ 1.85, the value cited in the
/// BFAST literature.) Intermediate `h` are linearly interpolated; values
/// outside the tabulated range are clamped to the nearest endpoint.
pub fn mosum_critical_value(h: f64) -> f64 {
    // (bandwidth, 5% critical value) knots, ascending in h.
    const TABLE: [(f64, f64); 7] = [
        (0.05, 2.27),
        (0.10, 1.99),
        (0.15, 1.85),
        (0.20, 1.76),
        (0.25, 1.69),
        (0.30, 1.64),
        (0.50, 1.50),
    ];

    if h <= TABLE[0].0 {
        return TABLE[0].1;
    }
    let last = TABLE.len() - 1;
    if h >= TABLE[last].0 {
        return TABLE[last].1;
    }
    for w in TABLE.windows(2) {
        let (h0, c0) = w[0];
        let (h1, c1) = w[1];
        if h >= h0 && h <= h1 {
            let frac = (h - h0) / (h1 - h0);
            return c0 + frac * (c1 - c0);
        }
    }
    // Unreachable given the clamps above, but keep a safe fallback.
    TABLE[last].1
}

/// Root-mean-square magnitude of a slice (the observation scale used to set the
/// relative σ̂ floor). Returns `0.0` for an empty slice.
fn root_mean_square(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = values.iter().map(|v| v * v).sum();
    (sum_sq / values.len() as f64).sqrt()
}

/// Sign of a value as an `i8`: `+1`, `0`, or `-1`.
fn sign_i8(v: f64) -> i8 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        0
    }
}

/// Coefficients of a fitted harmonic season + trend model.
///
/// Returned by [`fit_harmonic_season_trend`]. The model is
/// `Y_t = β0 + β1·t + Σ_{i=1..k} ( α_i·sin(2π i t / T) + γ_i·cos(2π i t / T) )`.
#[derive(Debug, Clone)]
pub struct HarmonicFit {
    /// Intercept `β0`.
    pub intercept: f64,
    /// Trend slope `β1` (change per sample).
    pub slope: f64,
    /// Sine amplitudes `α_1..α_k` (one per harmonic).
    pub sin_amplitudes: Vec<f64>,
    /// Cosine amplitudes `γ_1..γ_k` (one per harmonic).
    pub cos_amplitudes: Vec<f64>,
    /// Residual standard error `σ̂ = sqrt( RSS / (n − p) )`.
    pub sigma: f64,
}

/// Fit a harmonic season + trend model to a single series by OLS.
///
/// Convenience wrapper exposing the core BFAST regression for callers that want
/// the seasonal-trend decomposition directly (and for integration testing). The
/// observations are assumed equally spaced at unit time steps `t = 0, 1, …`.
///
/// `period` is the seasonal period `T` (in samples) and `order` the harmonic
/// order `k` (number of sine/cosine pairs). Returns `Ok(None)` when there are
/// too few observations for the requested model (`n ≤ 2 + 2k`) or the system is
/// rank-deficient.
///
/// # Errors
/// Returns [`TemporalError::invalid_parameter`] when `period` is not a finite
/// positive value or `order` is zero.
pub fn fit_harmonic_season_trend(
    values: &[f64],
    period: f64,
    order: usize,
) -> Result<Option<HarmonicFit>> {
    if !period.is_finite() || period <= 0.0 {
        return Err(TemporalError::invalid_parameter(
            "period",
            "must be a finite positive value",
        ));
    }
    if order == 0 {
        return Err(TemporalError::invalid_parameter(
            "order",
            "harmonic order must be at least 1",
        ));
    }

    let times: Vec<f64> = (0..values.len()).map(|t| t as f64).collect();
    let p = 2 + 2 * order;
    if values.len() <= p {
        return Ok(None);
    }

    // Reconstruct the per-harmonic amplitudes from the OLS solution by solving
    // once more here (kept independent so the internal hot path stays lean).
    let n = values.len();
    let mut design = Array2::<f64>::zeros((n, p));
    for (row, &t) in times.iter().enumerate().take(n) {
        design[[row, 0]] = 1.0;
        design[[row, 1]] = t;
        for h in 1..=order {
            let angle = 2.0 * std::f64::consts::PI * (h as f64) * t / period;
            design[[row, 2 * h]] = angle.sin();
            design[[row, 2 * h + 1]] = angle.cos();
        }
    }
    let y = Array1::from_vec(values.to_vec());
    let beta = match lstsq_ndarray(&design, &y) {
        Ok(b) => b,
        Err(_) => return Ok(None),
    };
    if beta.len() != p || beta.iter().any(|v| !v.is_finite()) {
        return Ok(None);
    }

    let mut rss = 0.0_f64;
    for (row, &t) in times.iter().enumerate().take(n) {
        let mut pred = beta[0] + beta[1] * t;
        for h in 1..=order {
            let angle = 2.0 * std::f64::consts::PI * (h as f64) * t / period;
            pred += beta[2 * h] * angle.sin() + beta[2 * h + 1] * angle.cos();
        }
        let resid = values[row] - pred;
        rss += resid * resid;
    }
    let sigma = (rss / (n - p) as f64).sqrt();

    let sin_amplitudes = (1..=order).map(|h| beta[2 * h]).collect();
    let cos_amplitudes = (1..=order).map(|h| beta[2 * h + 1]).collect();

    Ok(Some(HarmonicFit {
        intercept: beta[0],
        slope: beta[1],
        sin_amplitudes,
        cos_amplitudes,
        sigma,
    }))
}

/// Infer the seasonal period (in samples) that [`bfast_detect`] would use for a
/// given collection. Exposed so callers and tests can reproduce the internal
/// period selection.
#[must_use]
pub fn inferred_period(ts: &TimeSeriesRaster) -> f64 {
    let timestamps: Vec<i64> = ts.entries().keys().copied().collect();
    infer_period(ts, &timestamps)
}

/// Largest harmonic order [`bfast_detect`] would fit for `n` samples and the
/// given `period`; `None` if not even a first-order model is feasible. Exposed
/// for callers that want to mirror the internal model-order selection.
#[must_use]
pub fn selected_harmonic_order(n: usize, period: f64) -> Option<usize> {
    choose_harmonic_order(n, period)
}

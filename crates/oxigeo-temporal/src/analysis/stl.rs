//! Seasonal-Trend decomposition using Loess (STL)
//!
//! Implements the STL procedure of Cleveland, Cleveland, McRae & Terpenning
//! (1990). The decomposition recovers three additive components from an evenly
//! spaced time-series `Y_t`:
//!
//! ```text
//! Y_t = T_t + S_t + R_t
//! ```
//!
//! - `T_t` — the trend component, a slowly varying signal extracted by Loess.
//! - `S_t` — the seasonal component, periodic with period `p`.
//! - `R_t` — the remainder, the unexplained noise.
//!
//! The implementation follows the *inner loop* and *outer loop* recipes from
//! §3.5 of the reference. Robustness weights, when enabled, use the bisquare
//! function applied to the remainder of the previous iteration.
//!
//! # References
//!
//! - Cleveland, R. B., Cleveland, W. S., McRae, J. E., & Terpenning, I. (1990).
//!   STL: A seasonal-trend decomposition procedure based on Loess.
//!   *Journal of Official Statistics*, 6(1), 3-73.

use crate::analysis::loess::loess_smooth_indexed;
use crate::error::{Result, TemporalError};

/// Configuration knobs for the STL inner/outer loops.
#[derive(Debug, Clone)]
pub struct StlOptions {
    /// Length of one full season (e.g. 12 for monthly data with yearly cycle).
    pub period: usize,
    /// Width of the cycle-subseries Loess smoother. Per Cleveland 1990 this
    /// must be odd and at least 7; smaller values produce sharper seasonal
    /// signals at the expense of more residual noise.
    pub n_seasonal: usize,
    /// Width of the trend Loess smoother. Default uses the recipe from
    /// Cleveland 1990 §3.4: `next_odd(1.5 * period / (1 - 1.5 / n_seasonal))`.
    pub n_trend: usize,
    /// Width of the low-pass moving-average chain. Default = `next_odd(period)`.
    pub n_lowpass: usize,
    /// Number of inner-loop passes. Cleveland recommends 1–2; default 2.
    pub inner_iterations: usize,
    /// Number of outer-loop passes (robustness iterations). Default 0.
    pub outer_iterations: usize,
    /// Enable bisquare-weighted robust fitting. When `true` and
    /// `outer_iterations == 0`, the latter is forced to 5 (per Cleveland §3.5).
    pub robust: bool,
}

impl StlOptions {
    /// Construct default options for the supplied seasonal period.
    #[must_use]
    pub fn new(period: usize) -> Self {
        let n_seasonal = 7;
        let n_trend = default_n_trend(period, n_seasonal);
        let n_lowpass = next_odd(period as f64);
        Self {
            period,
            n_seasonal,
            n_trend,
            n_lowpass,
            inner_iterations: 2,
            outer_iterations: 0,
            robust: false,
        }
    }

    /// Override the cycle-subseries smoother width.
    #[must_use]
    pub fn with_n_seasonal(mut self, n_seasonal: usize) -> Self {
        self.n_seasonal = if n_seasonal.is_multiple_of(2) {
            n_seasonal + 1
        } else {
            n_seasonal.max(3)
        };
        self.n_trend = default_n_trend(self.period, self.n_seasonal);
        self
    }

    /// Override the trend smoother width.
    #[must_use]
    pub fn with_n_trend(mut self, n_trend: usize) -> Self {
        self.n_trend = if n_trend.is_multiple_of(2) {
            n_trend + 1
        } else {
            n_trend.max(3)
        };
        self
    }

    /// Override the low-pass smoother width.
    #[must_use]
    pub fn with_n_lowpass(mut self, n_lowpass: usize) -> Self {
        self.n_lowpass = if n_lowpass.is_multiple_of(2) {
            n_lowpass + 1
        } else {
            n_lowpass.max(3)
        };
        self
    }

    /// Override the inner-loop iteration count.
    #[must_use]
    pub fn with_inner_iterations(mut self, iterations: usize) -> Self {
        self.inner_iterations = iterations.max(1);
        self
    }

    /// Override the outer-loop iteration count.
    #[must_use]
    pub fn with_outer_iterations(mut self, iterations: usize) -> Self {
        self.outer_iterations = iterations;
        self
    }

    /// Enable robust (bisquare-reweighted) STL.
    #[must_use]
    pub fn with_robust(mut self) -> Self {
        self.robust = true;
        if self.outer_iterations == 0 {
            self.outer_iterations = 5;
        }
        self
    }
}

/// Result of running [`stl_decompose`].
#[derive(Debug, Clone)]
pub struct StlResult {
    /// Trend component, same length as the input series.
    pub trend: Vec<f64>,
    /// Seasonal component, same length as the input series.
    pub seasonal: Vec<f64>,
    /// Remainder `Y_t - T_t - S_t`.
    pub residual: Vec<f64>,
    /// Robustness weights from the final outer iteration (1.0 when robustness
    /// is disabled).
    pub robustness_weights: Vec<f64>,
}

/// Compute the canonical default trend window, `next_odd(1.5 * p / (1 - 1.5 /
/// n_s))`.
#[must_use]
pub fn default_n_trend(period: usize, n_seasonal: usize) -> usize {
    let p = period as f64;
    let ns = n_seasonal as f64;
    let denom = 1.0 - 1.5 / ns;
    if denom.abs() < f64::EPSILON {
        next_odd(1.5 * p)
    } else {
        next_odd(1.5 * p / denom)
    }
}

/// Return the smallest odd integer ≥ `x`. Always returns at least 1.
#[must_use]
pub fn next_odd(x: f64) -> usize {
    let n = x.ceil().max(1.0) as usize;
    if n.is_multiple_of(2) { n + 1 } else { n.max(1) }
}

/// Run STL on a 1D series.
///
/// # Errors
/// Returns an error when `values.len() < 2 * options.period` or
/// `options.period < 2`.
pub fn stl_decompose(values: &[f64], options: &StlOptions) -> Result<StlResult> {
    let n = values.len();
    let period = options.period;
    if period < 2 {
        return Err(TemporalError::invalid_parameter(
            "period",
            format!("STL requires period ≥ 2, got {}", period),
        ));
    }
    if n < 2 * period {
        return Err(TemporalError::insufficient_data(format!(
            "STL requires at least 2 full periods ({} observations), got {}",
            2 * period,
            n
        )));
    }
    if options.inner_iterations == 0 {
        return Err(TemporalError::invalid_parameter(
            "inner_iterations",
            "STL needs at least one inner iteration",
        ));
    }

    let mut trend = vec![0.0_f64; n];
    let mut seasonal = vec![0.0_f64; n];
    let mut robust_weights = vec![1.0_f64; n];

    let outer = options
        .outer_iterations
        .max(if options.robust { 1 } else { 0 });

    for outer_iter in 0..=outer {
        // ---- Inner loop ---- (Cleveland 1990 §3.5)
        for _ in 0..options.inner_iterations {
            // Step 1: detrend Y - T (T is zero on the very first iteration).
            let detrended: Vec<f64> = values
                .iter()
                .zip(trend.iter())
                .map(|(y, t)| y - t)
                .collect();

            // Step 2: cycle-subseries smoothing — for each phase `k` ∈ 0..period,
            // gather samples at positions k, k+p, k+2p, ... and Loess-smooth.
            // We then pad with one phantom value on each end so that the next
            // step's low-pass moving-averages have enough buffer (per the
            // §3.5 description).
            let c_padded = smooth_cycle_subseries(&detrended, period, options.n_seasonal);

            // Step 3: low-pass — three successive moving averages of length
            // period, period, 3 — then a Loess smoothing of width n_lowpass.
            let low_pass = low_pass_chain(&c_padded, period, options.n_lowpass);

            // Step 4: seasonal = c_padded[period..period+n] - low_pass
            for i in 0..n {
                seasonal[i] = c_padded[i + period] - low_pass[i];
            }

            // Step 5: deseasonalise.
            let deseasonalised: Vec<f64> = values
                .iter()
                .zip(seasonal.iter())
                .map(|(y, s)| y - s)
                .collect();

            // Step 6: trend smoothing.
            trend = loess_with_robustness(&deseasonalised, options.n_trend, &robust_weights);
        }

        // Final remainder for this outer iteration.
        let residual: Vec<f64> = values
            .iter()
            .zip(trend.iter())
            .zip(seasonal.iter())
            .map(|((y, t), s)| y - t - s)
            .collect();

        // Update robustness weights from the residual if we have outer passes left.
        if outer_iter < outer {
            robust_weights = compute_robustness_weights(&residual);
        } else {
            // Last pass: keep the weights from the previous iteration but
            // store the freshly computed residual.
            let final_residual = residual.clone();
            return Ok(StlResult {
                trend,
                seasonal,
                residual: final_residual,
                robustness_weights: robust_weights,
            });
        }
    }

    // Fall-through (should be unreachable because the loop returns on the
    // final iteration). Emit a residual just in case.
    let residual: Vec<f64> = values
        .iter()
        .zip(trend.iter())
        .zip(seasonal.iter())
        .map(|((y, t), s)| y - t - s)
        .collect();
    Ok(StlResult {
        trend,
        seasonal,
        residual,
        robustness_weights: robust_weights,
    })
}

/// Smooth each cycle-subseries with a Loess of width `n_seasonal`. The output
/// has length `n + 2 * period`, with one phantom period appended on either end
/// (Cleveland 1990 §3.5 "C-series").
fn smooth_cycle_subseries(detrended: &[f64], period: usize, n_seasonal: usize) -> Vec<f64> {
    let n = detrended.len();
    let mut padded = vec![0.0_f64; n + 2 * period];
    for k in 0..period {
        let mut sub = Vec::new();
        let mut idx = k;
        while idx < n {
            sub.push(detrended[idx]);
            idx += period;
        }
        if sub.is_empty() {
            continue;
        }
        let smoothed = loess_smooth_indexed(&sub, n_seasonal, 1);
        // Pad by one extra cycle on each end via linear extrapolation of the
        // first/last fitted values, as in Cleveland 1990.
        let first = smoothed[0];
        let last = smoothed[smoothed.len() - 1];
        // Fill into padded array.
        // Phantom front: index `k` in original = index `k + period` in padded.
        // Front phantom (subseries position -1) maps to padded index `k`.
        padded[k] = first;
        for (j, &v) in smoothed.iter().enumerate() {
            padded[k + period + j * period] = v;
        }
        // Back phantom: subseries position `len(sub)` → padded index `k + (len(sub)+1)*period`.
        let tail = k + (smoothed.len() + 1) * period;
        if tail < padded.len() {
            padded[tail] = last;
        }
    }
    padded
}

/// Apply the §3.5 low-pass: MA(period) ∘ MA(period) ∘ MA(3) ∘ Loess(n_lowpass).
fn low_pass_chain(c_padded: &[f64], period: usize, n_lowpass: usize) -> Vec<f64> {
    // The result must have length n = c_padded.len() - 2 * period.
    let total = c_padded.len();
    let stage1 = moving_average(c_padded, period);
    let stage2 = moving_average(&stage1, period);
    let stage3 = moving_average(&stage2, 3);
    // After three centred MAs the result is still aligned to stage3.len().
    // We slice the central `total - 2 * period` values, then Loess them.
    let n = total - 2 * period;
    let offset = (stage3.len() - n) / 2;
    let trimmed: Vec<f64> = stage3[offset..offset + n].to_vec();
    loess_smooth_indexed(&trimmed, n_lowpass, 1)
}

/// Centred moving average of `window` points. The output has length
/// `values.len() - window + 1`.
fn moving_average(values: &[f64], window: usize) -> Vec<f64> {
    let n = values.len();
    if window == 0 || window > n {
        return values.to_vec();
    }
    let mut out = Vec::with_capacity(n - window + 1);
    let mut sum: f64 = values.iter().take(window).copied().sum();
    out.push(sum / window as f64);
    for i in window..n {
        sum += values[i] - values[i - window];
        out.push(sum / window as f64);
    }
    out
}

fn loess_with_robustness(values: &[f64], window: usize, weights: &[f64]) -> Vec<f64> {
    let n = values.len();
    if weights.iter().all(|w| (*w - 1.0).abs() < f64::EPSILON) {
        return loess_smooth_indexed(values, window, 1);
    }
    // When robustness weights are present we run a single weighted Loess
    // pass using the higher-level API.
    use crate::analysis::loess::{LoessOptions, loess_smooth_1d};
    let x: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let frac = (window as f64 / n as f64).clamp(0.01, 1.0);
    let opts = LoessOptions {
        bandwidth_fraction: frac,
        degree: 1,
        robustness_iterations: 0,
        weights: Some(weights.to_vec()),
    };
    loess_smooth_1d(&x, values, &opts).unwrap_or_else(|_| values.to_vec())
}

/// Compute bisquare robustness weights from a residual vector.
fn compute_robustness_weights(residual: &[f64]) -> Vec<f64> {
    let mut abs: Vec<f64> = residual.iter().map(|r| r.abs()).collect();
    if abs.is_empty() {
        return Vec::new();
    }
    abs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = abs[abs.len() / 2];
    let scale = 6.0 * median;
    residual
        .iter()
        .map(|r| {
            if scale <= 0.0 {
                1.0
            } else {
                let u = r / scale;
                if u.abs() >= 1.0 {
                    0.0
                } else {
                    let v = 1.0 - u * u;
                    (v * v).clamp(0.0, 1.0)
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_odd_basic() {
        assert_eq!(next_odd(3.0), 3);
        assert_eq!(next_odd(4.0), 5);
        assert_eq!(next_odd(4.5), 5);
        assert_eq!(next_odd(0.0), 1);
    }

    #[test]
    fn default_n_trend_matches_recipe() {
        let p = 12;
        let ns = 7;
        let want = next_odd(1.5 * p as f64 / (1.0 - 1.5 / ns as f64));
        assert_eq!(default_n_trend(p, ns), want);
    }

    #[test]
    fn stl_short_input_errors() {
        let opts = StlOptions::new(12);
        let res = stl_decompose(&[1.0, 2.0, 3.0], &opts);
        assert!(res.is_err());
    }
}

//! LandTrendr piecewise-linear segmentation.
//!
//! Reference: Kennedy, R.E., Yang, Z., Cohen, W.B. (2010).
//! "Detecting trends in forest disturbance and recovery using yearly Landsat
//! time series: 1. LandTrendr - Temporal segmentation algorithms."
//! *Remote Sensing of Environment* 114(12): 2897-2910.
//!
//! This module exposes [`landtrendr_segment`] which fits a sequence of
//! piecewise-linear segments to a one-dimensional time series, returning the
//! optimal vertex set. The implementation follows the algorithm outlined in
//! Kennedy et al. (2010) §2.2:
//!
//! 1. Spike pre-processing (single-pass desparkle).
//! 2. Initial vertex set built from equally-spaced indices using an overshoot
//!    relative to `max_segments`.
//! 3. Iterative vertex removal, each time greedily dropping the interior
//!    vertex whose removal causes the smallest MSE increase. After every
//!    removal all segments are re-fit through point-to-point regression
//!    anchored at the remaining vertices.
//! 4. Model selection by F-statistic walk: starting at the coarsest two-vertex
//!    model and walking towards more complex models, pick the *simplest* model
//!    whose comparison F-statistic with the next-larger model falls below
//!    `F_crit * best_model_proportion`.
//! 5. Optional "prevent one-year recovery" pass to merge isolated 1-step
//!    positive-slope segments.
//! 6. Optional recovery-threshold filter that drops terminal recovery
//!    segments whose magnitude is below `recovery_threshold * (max - min)`.
//!
//! All public types use [`scirs2_core::ndarray`] (per the SCIRS2 policy).

use crate::error::{Result, TemporalError};
use scirs2_core::linalg::lstsq_ndarray;
use scirs2_core::ndarray::{Array1, Array2};

/// Critical value of the F distribution used by the model-selection walk.
///
/// Kennedy 2010 §2.2.3 prescribes the actual F-critical for the degrees of
/// freedom of the candidate models. As a documented simplification we use a
/// fixed value (2.0) corresponding to the upper-tail critical value of an
/// F(1, ~20) distribution at p ≈ 0.05 — this is appropriate for the typical
/// time-series lengths (10-40 observations) targeted by LandTrendr.
const F_CRIT_APPROX: f64 = 2.0;

/// LandTrendr segmentation options.
///
/// Defaults follow Kennedy et al. (2010) Table 1.
#[derive(Debug, Clone)]
pub struct LandTrendrOptions {
    /// Maximum number of segments allowed in the final model (Kennedy 2010 §2.2.2).
    pub max_segments: usize,
    /// Spike-detection threshold expressed as a fraction of total dynamic range.
    /// A value `t` of `0.9` flags points whose distance from the maximum of
    /// their immediate neighbours exceeds 90 % of the series amplitude.
    pub spike_threshold: f64,
    /// Number of extra vertices kept in the initial vertex set on top of
    /// `max_segments + 1`. Provides degrees of freedom for the removal pass.
    pub vertex_count_overshoot: usize,
    /// If `true`, enforces the "prevent one-year recovery" rule that merges
    /// any positive-slope segment of length 1 with its neighbours.
    pub prevent_one_year_recovery: bool,
    /// Drop terminal recovery segments whose magnitude is less than this
    /// fraction of the series dynamic range.
    pub recovery_threshold: f64,
    /// Critical p-value retained for documentation. The model-selection walk
    /// actually uses `F_CRIT_APPROX` as the F-critical surrogate.
    pub pval_threshold: f64,
    /// Fraction of `F_CRIT_APPROX` below which a more-complex model is *not*
    /// preferred over a simpler one (Kennedy 2010 §2.2.3 "best-model
    /// proportion").
    pub best_model_proportion: f64,
    /// Minimum number of valid observations required to run the algorithm.
    pub min_observations: usize,
}

impl Default for LandTrendrOptions {
    fn default() -> Self {
        Self {
            max_segments: 6,
            spike_threshold: 0.9,
            vertex_count_overshoot: 3,
            prevent_one_year_recovery: true,
            recovery_threshold: 0.25,
            pval_threshold: 0.05,
            best_model_proportion: 0.75,
            min_observations: 6,
        }
    }
}

/// A single LandTrendr vertex: time index plus fitted value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandTrendrVertex {
    /// Index of the vertex along the time axis (0-based).
    pub year_idx: usize,
    /// Fitted value at the vertex.
    pub value: f64,
}

/// A LandTrendr segment connecting two consecutive vertices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandTrendrSegment {
    /// Time index of the segment start (inclusive).
    pub start_year: usize,
    /// Time index of the segment end (inclusive).
    pub end_year: usize,
    /// Fitted value at the start vertex.
    pub start_value: f64,
    /// Fitted value at the end vertex.
    pub end_value: f64,
}

impl LandTrendrSegment {
    /// Length of the segment along the time axis (`end - start`).
    #[must_use]
    pub fn length(&self) -> usize {
        self.end_year.saturating_sub(self.start_year)
    }

    /// Signed magnitude `end_value - start_value`.
    #[must_use]
    pub fn signed_magnitude(&self) -> f64 {
        self.end_value - self.start_value
    }

    /// Per-step slope (zero if length is zero).
    #[must_use]
    pub fn slope(&self) -> f64 {
        let len = self.length();
        if len == 0 {
            0.0
        } else {
            (self.end_value - self.start_value) / len as f64
        }
    }
}

/// Final LandTrendr segmentation result.
#[derive(Debug, Clone)]
pub struct LandTrendrResult {
    /// Optimal vertices selected by the model-selection walk.
    pub vertices: Vec<LandTrendrVertex>,
    /// Segments connecting consecutive vertices.
    pub segments: Vec<LandTrendrSegment>,
    /// Mean squared error of the chosen model on the (possibly desparkled)
    /// input series.
    pub mse: f64,
    /// F-statistic of the chosen model relative to the next-more-complex
    /// candidate (or `0.0` when the chosen model is the most complex one).
    pub p_of_f: f64,
}

/// Run LandTrendr piecewise-linear segmentation on a single 1D series.
///
/// # Errors
///
/// Returns [`TemporalError::insufficient_data`] when `values.len() <
/// options.min_observations`, or [`TemporalError::invalid_input`] when any
/// `values` entry is non-finite (`NaN` / `±Inf`).
pub fn landtrendr_segment(values: &[f64], options: &LandTrendrOptions) -> Result<LandTrendrResult> {
    if values.len() < options.min_observations {
        return Err(TemporalError::insufficient_data(format!(
            "LandTrendr requires at least {} observations, got {}",
            options.min_observations,
            values.len()
        )));
    }
    if values.iter().any(|v| !v.is_finite()) {
        return Err(TemporalError::invalid_input(
            "LandTrendr input contains non-finite values (NaN/Inf)",
        ));
    }
    if options.max_segments < 1 {
        return Err(TemporalError::invalid_parameter(
            "max_segments",
            "must be at least 1",
        ));
    }
    if !(0.0..=1.0).contains(&options.best_model_proportion) {
        return Err(TemporalError::invalid_parameter(
            "best_model_proportion",
            "must lie in [0, 1]",
        ));
    }

    // Step 1: spike pre-processing
    let mut work = values.to_vec();
    desparkle(&mut work, options.spike_threshold);

    // Step 2: initial vertex set via iterative bisection.
    let initial_indices = build_initial_vertex_indices(&work, options);
    let mut candidates: Vec<Vec<usize>> = Vec::new();

    // Refit at the initial vertex count, then iteratively drop vertices.
    let mut current_indices = initial_indices.clone();
    candidates.push(current_indices.clone());
    while current_indices.len() > 2 {
        let next = remove_least_important_vertex(&work, &current_indices)?;
        current_indices = next;
        candidates.push(current_indices.clone());
    }

    // Step 4: model selection via F-statistic walk.
    let (chosen_indices, chosen_mse, chosen_f) = select_best_model(&work, &candidates, options)?;

    // Build vertex + segment representation using LSQ-fitted anchor values.
    let fitted_values = fit_vertex_values(&work, &chosen_indices);
    let mut vertices: Vec<LandTrendrVertex> = chosen_indices
        .iter()
        .zip(fitted_values.iter())
        .map(|(&year_idx, &value)| LandTrendrVertex { year_idx, value })
        .collect();
    let mut segments = build_segments(&vertices);

    // Step 5: prevent-one-year-recovery
    if options.prevent_one_year_recovery {
        prevent_one_year_recovery(&mut vertices, &mut segments);
    }

    // Step 6: recovery-threshold filter
    apply_recovery_threshold(&mut vertices, &mut segments, &work, options);

    Ok(LandTrendrResult {
        vertices,
        segments,
        mse: chosen_mse,
        p_of_f: chosen_f,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Single-pass spike removal (Kennedy 2010 §2.2.1).
fn desparkle(values: &mut [f64], threshold: f64) {
    let n = values.len();
    if n < 3 {
        return;
    }
    let (mut min_v, mut max_v) = (f64::INFINITY, f64::NEG_INFINITY);
    for &v in values.iter() {
        if v < min_v {
            min_v = v;
        }
        if v > max_v {
            max_v = v;
        }
    }
    let range = (max_v - min_v).max(1e-9);
    let original = values.to_vec();
    for t in 1..n - 1 {
        let left = original[t - 1];
        let right = original[t + 1];
        let neighbour_max = left.max(right);
        let neighbour_min = left.min(right);
        let dist_high = (original[t] - neighbour_max).max(0.0);
        let dist_low = (neighbour_min - original[t]).max(0.0);
        let dist = dist_high.max(dist_low);
        if dist / range > threshold {
            values[t] = 0.5 * (left + right);
        }
    }
}

/// Build the initial vertex index set via iterative bisection (Kennedy et al.
/// 2007; Kennedy 2010 §2.2.2). The algorithm starts with `[0, n-1]` and
/// repeatedly inserts the index whose residual from the current piecewise-
/// linear fit is largest. This naturally finds breakpoints in step-like data
/// in addition to producing a candidate vertex per anticipated segment.
fn build_initial_vertex_indices(values: &[f64], options: &LandTrendrOptions) -> Vec<usize> {
    let n = values.len();
    if n <= 2 {
        return (0..n).collect();
    }
    let target_vertex_count = options
        .max_segments
        .saturating_add(1)
        .saturating_add(options.vertex_count_overshoot);
    let target = target_vertex_count.min(n).max(2);
    if target == n {
        return (0..n).collect();
    }

    let mut indices = vec![0_usize, n - 1];
    while indices.len() < target {
        // Build current piecewise-linear fit using the point-to-point line
        // through observed values at the existing anchor indices (this is the
        // bisection criterion; LSQ refinement happens later).
        let mut max_resid: f64 = -1.0;
        let mut best_idx: Option<usize> = None;
        for seg in 0..indices.len() - 1 {
            let i0 = indices[seg];
            let i1 = indices[seg + 1];
            if i1 <= i0 + 1 {
                continue;
            }
            let y0 = values[i0];
            let y1 = values[i1];
            let span = (i1 - i0) as f64;
            for (i, &v) in values.iter().enumerate().take(i1).skip(i0 + 1) {
                let t = (i - i0) as f64 / span;
                let fit = y0 + t * (y1 - y0);
                let r = (v - fit).abs();
                if r > max_resid {
                    max_resid = r;
                    best_idx = Some(i);
                }
            }
        }
        match best_idx {
            Some(idx) => {
                indices.push(idx);
                indices.sort_unstable();
                indices.dedup();
            }
            None => break,
        }
    }
    indices
}

/// Greedy vertex removal: drop the interior vertex whose removal yields the
/// smallest MSE increase under LSQ refit on the remaining vertices.
fn remove_least_important_vertex(values: &[f64], indices: &[usize]) -> Result<Vec<usize>> {
    if indices.len() <= 2 {
        return Ok(indices.to_vec());
    }
    let baseline_mse = piecewise_linear_mse(values, indices);
    let mut best_idx_to_remove: Option<usize> = None;
    let mut best_delta = f64::INFINITY;
    for k in 1..indices.len() - 1 {
        let mut trial: Vec<usize> = indices.to_vec();
        trial.remove(k);
        let mse = piecewise_linear_mse(values, &trial);
        let delta = mse - baseline_mse;
        if delta < best_delta {
            best_delta = delta;
            best_idx_to_remove = Some(k);
        }
    }
    let k = best_idx_to_remove.ok_or_else(|| {
        TemporalError::change_detection_error("LandTrendr removal pass found no candidate")
    })?;
    let mut out = indices.to_vec();
    out.remove(k);
    Ok(out)
}

/// Solve for the per-vertex anchor values that minimise the squared residual
/// of a piecewise-linear function with breakpoints at `indices` through the
/// observed `values` (Kennedy 2010 §2.2.2).
///
/// This is the proper LSQ formulation: anchor values are *unknown parameters*
/// fitted to minimise `||X β - y||²`, where `X` is the partition-of-unity
/// design matrix produced by [`build_anchored_design_matrix`].
fn fit_vertex_values(values: &[f64], indices: &[usize]) -> Vec<f64> {
    let n = values.len();
    let p = indices.len();
    if p == 0 || n == 0 {
        return Vec::new();
    }
    if p == 1 {
        return vec![values[indices[0]]];
    }
    let design = build_anchored_design_matrix(n, indices);
    let y = Array1::from(values.to_vec());
    match lstsq_ndarray(&design, &y) {
        Ok(beta) => {
            let mut out = vec![0.0_f64; p];
            for (i, slot) in out.iter_mut().enumerate().take(p) {
                *slot = beta[i];
            }
            out
        }
        Err(_) => {
            // Degenerate design (e.g. duplicate indices) — fall back to
            // point-to-point through raw observations.
            indices.iter().map(|&idx| values[idx]).collect()
        }
    }
}

/// Compute the MSE of an LSQ piecewise-linear fit through `indices`.
fn piecewise_linear_mse(values: &[f64], indices: &[usize]) -> f64 {
    if indices.len() < 2 {
        return f64::INFINITY;
    }
    let fitted = piecewise_linear_fit(values, indices);
    let n = values.len() as f64;
    if n <= 0.0 {
        return f64::INFINITY;
    }
    let mut sse = 0.0;
    for (i, &v) in values.iter().enumerate() {
        let r = fitted[i] - v;
        sse += r * r;
    }
    sse / n
}

/// Evaluate a piecewise-linear function with LSQ-fitted anchor values at
/// every sample index.
fn piecewise_linear_fit(values: &[f64], indices: &[usize]) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![0.0; n];
    if indices.len() < 2 {
        let v = values.first().copied().unwrap_or(0.0);
        for slot in out.iter_mut() {
            *slot = v;
        }
        return out;
    }
    let anchor_values = fit_vertex_values(values, indices);
    for seg in 0..indices.len() - 1 {
        let i0 = indices[seg];
        let i1 = indices[seg + 1];
        let y0 = anchor_values[seg];
        let y1 = anchor_values[seg + 1];
        let span = if i1 > i0 { (i1 - i0) as f64 } else { 1.0 };
        for (i, slot) in out.iter_mut().enumerate().take(i1 + 1).skip(i0) {
            let t = (i - i0) as f64 / span;
            *slot = y0 + t * (y1 - y0);
        }
    }
    // Defensive: fill any indices outside [first, last] anchor span with the
    // closest anchor value (cannot happen when first=0 / last=n-1).
    if let Some(&first) = indices.first() {
        let v = anchor_values[0];
        for slot in out.iter_mut().take(first) {
            *slot = v;
        }
    }
    if let Some(&last) = indices.last() {
        let v = *anchor_values.last().unwrap_or(&0.0);
        for slot in out.iter_mut().skip(last + 1) {
            *slot = v;
        }
    }
    out
}

/// Sum of squared residuals (LSQ-fit).
fn sum_squared_residuals(values: &[f64], indices: &[usize]) -> f64 {
    let fitted = piecewise_linear_fit(values, indices);
    let mut sse = 0.0;
    for (i, &v) in values.iter().enumerate() {
        let r = fitted[i] - v;
        sse += r * r;
    }
    sse
}

/// Model-selection (Kennedy 2010 §2.2.3).
///
/// `candidates` is ordered from most complex (longest index vector) to least
/// complex (length 2). For every candidate we compute the F-statistic
/// *relative to the most-complex (full) candidate*:
///
/// ```text
/// F = ((SSE_candidate - SSE_full) / (p_full - p_candidate))
///   / ( SSE_full / (n - p_full) )
/// ```
///
/// We then pick the *simplest* candidate whose F is below `F_CRIT_APPROX *
/// best_model_proportion`, which means the extra parameters of the full
/// model do not significantly improve the fit. If every candidate is
/// significantly worse than the full model, we keep the full model.
fn select_best_model(
    values: &[f64],
    candidates: &[Vec<usize>],
    options: &LandTrendrOptions,
) -> Result<(Vec<usize>, f64, f64)> {
    if candidates.is_empty() {
        return Err(TemporalError::change_detection_error(
            "LandTrendr produced no candidate models",
        ));
    }
    // Filter to those with vertex-count ≤ max_segments + 1.
    let allowed_max_vertices = options.max_segments.saturating_add(1);
    let mut filtered: Vec<&Vec<usize>> = candidates
        .iter()
        .filter(|c| c.len() <= allowed_max_vertices)
        .collect();
    if filtered.is_empty() {
        // Fall back to the simplest candidate.
        let last_idx = candidates.len() - 1;
        let chosen = candidates[last_idx].clone();
        let mse = piecewise_linear_mse(values, &chosen);
        return Ok((chosen, mse, 0.0));
    }
    let n = values.len();
    let threshold = F_CRIT_APPROX * options.best_model_proportion;

    // After filter, `filtered` is most-complex-first.
    // The first entry is the "full" model.
    let full = filtered[0].clone();
    let sse_full = sum_squared_residuals(values, &full);
    let p_full = full.len();
    let df_full = n.saturating_sub(p_full);

    // If the full model is already a perfect fit (or essentially so), accept
    // the simplest two-vertex model that *also* fits perfectly. Otherwise we
    // fall through to the standard F-walk.
    let total_var: f64 = {
        let mean = values.iter().copied().sum::<f64>() / n as f64;
        values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>()
    };
    let perfect_fit_floor = total_var.max(1.0) * 1e-10;

    // Sort filtered simplest-to-most-complex so we can return the simplest
    // adequate model.
    filtered.reverse();

    let mut chosen: Vec<usize> = full.clone();
    let mut chosen_f: f64 = 0.0;
    for cand in &filtered {
        let p_cand = cand.len();
        let sse_cand = sum_squared_residuals(values, cand);
        // If this candidate already perfectly explains the data, pick it.
        if sse_cand <= perfect_fit_floor {
            chosen = (*cand).clone();
            chosen_f = 0.0;
            break;
        }
        if p_full <= p_cand {
            // Same complexity as full: nothing simpler is possible.
            chosen = (*cand).clone();
            chosen_f = 0.0;
            break;
        }
        if df_full == 0 {
            break;
        }
        let df_num = p_full - p_cand;
        let numerator = (sse_cand - sse_full).max(0.0) / df_num as f64;
        let denominator = sse_full / df_full as f64;
        let f_stat = if denominator > 0.0 {
            numerator / denominator
        } else if numerator > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };
        if f_stat <= threshold {
            // The simpler model is statistically as good as the full one.
            chosen = (*cand).clone();
            chosen_f = f_stat;
            break;
        }
    }
    let mse = piecewise_linear_mse(values, &chosen);
    Ok((chosen, mse, chosen_f))
}

/// Translate a vector of vertices to consecutive segments.
fn build_segments(vertices: &[LandTrendrVertex]) -> Vec<LandTrendrSegment> {
    let mut out = Vec::with_capacity(vertices.len().saturating_sub(1));
    for w in vertices.windows(2) {
        let a = w[0];
        let b = w[1];
        out.push(LandTrendrSegment {
            start_year: a.year_idx,
            end_year: b.year_idx,
            start_value: a.value,
            end_value: b.value,
        });
    }
    out
}

/// Merge any positive-slope segment of length 1 with its right neighbour.
fn prevent_one_year_recovery(
    vertices: &mut Vec<LandTrendrVertex>,
    segments: &mut Vec<LandTrendrSegment>,
) {
    if vertices.len() <= 2 {
        return;
    }
    let mut i = 0;
    while i < segments.len() {
        let seg = segments[i];
        let len = seg.length();
        let slope_positive = seg.end_value > seg.start_value;
        if len == 1 && slope_positive && i + 1 < segments.len() {
            // Remove the vertex at the *right* of this segment (i.e. vertex
            // index `i + 1` in the `vertices` array).
            let remove_at = i + 1;
            if remove_at < vertices.len() - 1 {
                vertices.remove(remove_at);
                *segments = build_segments(vertices);
                continue;
            }
        }
        i += 1;
    }
}

/// Drop a terminal "recovery" segment whose magnitude is below the recovery
/// threshold expressed as a fraction of the input dynamic range.
fn apply_recovery_threshold(
    vertices: &mut Vec<LandTrendrVertex>,
    segments: &mut Vec<LandTrendrSegment>,
    values: &[f64],
    options: &LandTrendrOptions,
) {
    if segments.is_empty() || vertices.len() <= 2 {
        return;
    }
    let mut min_v = f64::INFINITY;
    let mut max_v = f64::NEG_INFINITY;
    for &v in values.iter() {
        if v < min_v {
            min_v = v;
        }
        if v > max_v {
            max_v = v;
        }
    }
    let dyn_range = (max_v - min_v).max(1e-9);
    let cutoff = options.recovery_threshold * dyn_range;
    let last = match segments.last() {
        Some(s) => *s,
        None => return,
    };
    if last.signed_magnitude() > 0.0 && last.signed_magnitude().abs() < cutoff {
        // Drop terminal recovery: remove the penultimate vertex so the last
        // segment is folded into the previous one, OR drop the trailing
        // vertex if only two vertices remain (degenerates to no segments).
        if vertices.len() >= 3 {
            let last_idx = vertices.len() - 2;
            vertices.remove(last_idx);
            *segments = build_segments(vertices);
        }
    }
}

// ---------------------------------------------------------------------------
// Optional: matrix-form least-squares utilities (kept for documentation and
// for callers wanting the full anchored regression). These intentionally use
// `scirs2_core::ndarray` per the SCIRS2 policy.
// ---------------------------------------------------------------------------

/// Build the design matrix for a piecewise-linear fit anchored at `indices`.
///
/// Each column corresponds to a vertex and each row to a sample index `i`.
/// Within segment `[i_k, i_{k+1}]` the design row interpolates linearly
/// between the two anchor columns:
/// * column `k`     receives weight `(i_{k+1} - i) / (i_{k+1} - i_k)`
/// * column `k+1`   receives weight `(i - i_k)     / (i_{k+1} - i_k)`
///
/// All other columns are zero. Outside the anchored range the row simply
/// activates the nearest anchor with weight `1.0` (rare in practice as we
/// always include `0` and `n-1`).
#[must_use]
pub fn build_anchored_design_matrix(n: usize, indices: &[usize]) -> Array2<f64> {
    let p = indices.len();
    let mut design = Array2::<f64>::zeros((n, p));
    if p == 0 {
        return design;
    }
    if p == 1 {
        for i in 0..n {
            design[[i, 0]] = 1.0;
        }
        return design;
    }
    for i in 0..n {
        // Find which segment `i` belongs to.
        let mut seg = 0_usize;
        while seg + 1 < p && indices[seg + 1] < i {
            seg += 1;
        }
        let i0 = indices[seg.min(p - 1)];
        let i1 = indices[(seg + 1).min(p - 1)];
        if i1 == i0 {
            design[[i, seg.min(p - 1)]] = 1.0;
            continue;
        }
        if i <= i0 {
            design[[i, seg]] = 1.0;
            continue;
        }
        if i >= i1 {
            design[[i, seg + 1]] = 1.0;
            continue;
        }
        let span = (i1 - i0) as f64;
        let w_right = (i - i0) as f64 / span;
        let w_left = 1.0 - w_right;
        design[[i, seg]] = w_left;
        design[[i, seg + 1]] = w_right;
    }
    design
}

/// Convert a slice of values into a [`scirs2_core::ndarray::Array1`].
#[must_use]
pub fn values_to_array1(values: &[f64]) -> Array1<f64> {
    Array1::from(values.to_vec())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_match_kennedy_2010() {
        let opts = LandTrendrOptions::default();
        assert_eq!(opts.max_segments, 6);
        assert!((opts.spike_threshold - 0.9).abs() < 1e-12);
        assert_eq!(opts.vertex_count_overshoot, 3);
        assert!(opts.prevent_one_year_recovery);
        assert!((opts.recovery_threshold - 0.25).abs() < 1e-12);
        assert!((opts.pval_threshold - 0.05).abs() < 1e-12);
        assert!((opts.best_model_proportion - 0.75).abs() < 1e-12);
        assert_eq!(opts.min_observations, 6);
    }

    #[test]
    fn constant_series_yields_two_vertices() {
        let values = vec![5.0; 20];
        let opts = LandTrendrOptions::default();
        let res = landtrendr_segment(&values, &opts).expect("segmentation");
        assert!(res.vertices.len() >= 2);
        let first = res.vertices.first().expect("first").value;
        let last = res.vertices.last().expect("last").value;
        assert!((first - 5.0).abs() < 1e-9);
        assert!((last - 5.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_short_series() {
        let values = vec![1.0, 2.0, 3.0];
        let opts = LandTrendrOptions::default();
        let err = landtrendr_segment(&values, &opts).expect_err("should fail");
        assert!(
            matches!(err, TemporalError::InsufficientData(_)),
            "expected InsufficientData, got {err:?}"
        );
    }

    #[test]
    fn rejects_non_finite_values() {
        let mut values = vec![1.0; 12];
        values[3] = f64::NAN;
        let opts = LandTrendrOptions::default();
        let err = landtrendr_segment(&values, &opts).expect_err("should fail");
        assert!(
            matches!(err, TemporalError::InvalidInput(_)),
            "expected InvalidInput, got {err:?}"
        );
    }

    #[test]
    fn desparkle_dampens_outliers() {
        let mut series = vec![1.0, 1.0, 1.0, 10.0, 1.0, 1.0, 1.0];
        desparkle(&mut series, 0.5);
        assert!((series[3] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn build_segments_is_consistent_with_vertices() {
        let vs = vec![
            LandTrendrVertex {
                year_idx: 0,
                value: 1.0,
            },
            LandTrendrVertex {
                year_idx: 5,
                value: 4.0,
            },
            LandTrendrVertex {
                year_idx: 10,
                value: 2.0,
            },
        ];
        let segs = build_segments(&vs);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].end_value, segs[1].start_value);
    }

    #[test]
    fn design_matrix_partition_of_unity() {
        let indices = vec![0_usize, 4, 9];
        let design = build_anchored_design_matrix(10, &indices);
        for i in 0..10 {
            let row_sum: f64 = (0..indices.len()).map(|j| design[[i, j]]).sum();
            assert!(
                (row_sum - 1.0).abs() < 1e-9,
                "row {} sums to {}",
                i,
                row_sum
            );
        }
    }
}

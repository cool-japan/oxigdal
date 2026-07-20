//! In-browser anomaly detection for the GeoVault workstation.
//!
//! This module is deliberately **self-contained**: it does not link
//! `oxigeo-analytics`, `oxigeo-qc`, or `scirs2` into the WASM bundle.
//! Instead it ports the exact scalar kernels, so the numbers produced in the
//! browser match the native crates:
//!
//! * `oxigeo-analytics/src/timeseries/anomaly.rs`
//!   - `z_score` (lines 90–114): population σ, `z = (x − mean) / σ`
//!   - `iqr_score` (lines 119–147): `score = |x − median| / (q3 − q1)`
//!   - `modified_z_score` (lines 152–186):
//!     `score = 0.6745·(x − median) / (1.4826·MAD)`
//!   - `percentile` (lines 196–220): linear interpolation between closest
//!     ranks over sorted data
//!   - threshold rule in `detect` (lines 66–77): a sample is anomalous when
//!     `score.abs() >= threshold` (boundary cases included)
//! * `oxigeo-qc/src/raster/consistency.rs`
//!   - `detect_outliers` σ-bounds (lines 441–442):
//!     `lower = mean − k·σ`, `upper = mean + k·σ`
//!   - outlier-percentage rule (lines 458–462):
//!     `count / valid_count · 100` (zero when there are no valid pixels)
//!   - σ is the **population** standard deviation, as computed by
//!     `RasterBuffer::compute_statistics` in `oxigeo-core/src/buffer/mod.rs`
//!     (lines 955–957) — identical to the analytics `z_score` variance
//!
//! Wasm-surface adaptations (the arithmetic itself is untouched):
//!
//! * Input is a raster band (`&[f32]` plus a nodata sentinel) instead of an
//!   `Array1<f64>` time series. NaN / non-finite / nodata samples are
//!   filtered out **before** any statistic is computed, mirroring the
//!   `!is_nodata(value) && value.is_finite()` guards used throughout
//!   `oxigeo-qc`. Pass `NaN` as the nodata value when the raster has none.
//! * Where the native detectors return `AnalyticsError` (insufficient data,
//!   near-zero spread) this port degrades gracefully instead of erroring:
//!   the mask flags nothing (all zeros) and scores are NaN. A constant
//!   raster therefore never panics and never reports anomalies.

use wasm_bindgen::prelude::*;
use web_sys::ImageData;

/// Consistency constant relating MAD to σ under a normal distribution.
///
/// Verbatim from `oxigeo-analytics/src/timeseries/anomaly.rs:171`.
const MAD_CONSISTENCY: f64 = 1.4826;

/// Scale factor of the modified Z-score (Iglewicz–Hoaglin).
///
/// Verbatim from `oxigeo-analytics/src/timeseries/anomaly.rs:181`.
const MODIFIED_Z_SCALE: f64 = 0.6745;

/// Anomaly-detection method selector (mirrors `AnomalyMethod` in
/// `oxigeo-analytics`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    /// Standard Z-score (population σ). Assumes a normal distribution.
    ZScore,
    /// Interquartile-range score. Robust to outliers.
    Iqr,
    /// Modified Z-score using the median absolute deviation. Most robust.
    ModifiedZ,
}

impl Method {
    /// Parses a JS-facing method name (case-insensitive, tolerant of
    /// `-`/`_` variants). Returns `None` for unknown names.
    fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "zscore" | "z-score" | "z_score" => Some(Self::ZScore),
            "iqr" => Some(Self::Iqr),
            "modified_zscore" | "modified-zscore" | "modifiedzscore" | "modified_z" => {
                Some(Self::ModifiedZ)
            }
            _ => None,
        }
    }

    /// Canonical name used in summaries regardless of the alias supplied.
    fn canonical_name(self) -> &'static str {
        match self {
            Self::ZScore => "zscore",
            Self::Iqr => "iqr",
            Self::ModifiedZ => "modified_zscore",
        }
    }
}

/// Returns `true` when the sample participates in statistics: finite and not
/// equal to the nodata sentinel (`nodata = NaN` disables the sentinel test,
/// because `v != NaN` is always true).
#[inline]
fn is_valid(value: f32, nodata: f32) -> bool {
    value.is_finite() && value != nodata
}

/// Percentile of pre-sorted data by linear interpolation between closest
/// ranks.
///
/// Verbatim port of `percentile` from
/// `oxigeo-analytics/src/timeseries/anomaly.rs:196-220` with the error
/// return replaced by `None` (empty data / percentile outside `0..=100`).
fn percentile(sorted_data: &[f64], percentile: f64) -> Option<f64> {
    if sorted_data.is_empty() {
        return None;
    }
    if !(0.0..=100.0).contains(&percentile) {
        return None;
    }

    let n = sorted_data.len();
    if n == 1 {
        return Some(sorted_data[0]);
    }

    // Linear interpolation between closest ranks.
    let rank = (percentile / 100.0) * ((n - 1) as f64);
    let lower_idx = rank.floor() as usize;
    let upper_idx = rank.ceil() as usize;
    let fraction = rank - (lower_idx as f64);

    Some(sorted_data[lower_idx] + fraction * (sorted_data[upper_idx] - sorted_data[lower_idx]))
}

/// Z-scores of `values` (population σ).
///
/// Verbatim port of `z_score` from
/// `oxigeo-analytics/src/timeseries/anomaly.rs:90-114`; returns `None`
/// where the source returns `AnalyticsError` (fewer than two samples, or a
/// standard deviation below `f64::EPSILON`).
fn zscore_scores(values: &[f64]) -> Option<Vec<f64>> {
    if values.len() < 2 {
        return None;
    }

    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let std = variance.sqrt();

    if std < f64::EPSILON {
        return None;
    }

    Some(values.iter().map(|value| (value - mean) / std).collect())
}

/// IQR scores of `values` (`|x − median| / iqr`).
///
/// Verbatim port of `iqr_score` from
/// `oxigeo-analytics/src/timeseries/anomaly.rs:119-147`; returns `None`
/// where the source returns `AnalyticsError` (fewer than four samples, or an
/// IQR below `f64::EPSILON`).
fn iqr_scores(values: &[f64]) -> Option<Vec<f64>> {
    if values.len() < 4 {
        return None;
    }

    // Sort values for percentile calculation.
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let q1 = percentile(&sorted, 25.0)?;
    let q3 = percentile(&sorted, 75.0)?;
    let iqr = q3 - q1;

    if iqr < f64::EPSILON {
        return None;
    }

    let median = percentile(&sorted, 50.0)?;

    // Distance from the median in units of IQR.
    Some(
        values
            .iter()
            .map(|value| (value - median).abs() / iqr)
            .collect(),
    )
}

/// Modified Z-scores of `values` (median absolute deviation based).
///
/// Verbatim port of `modified_z_score` from
/// `oxigeo-analytics/src/timeseries/anomaly.rs:152-186`; returns `None`
/// where the source returns `AnalyticsError` (fewer than two samples, or a
/// normalized MAD below `f64::EPSILON`).
fn modified_zscore_scores(values: &[f64]) -> Option<Vec<f64>> {
    if values.len() < 2 {
        return None;
    }

    // Sort values for median calculation.
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median = percentile(&sorted, 50.0)?;

    // Median absolute deviation.
    let mut abs_deviations: Vec<f64> = values.iter().map(|x| (x - median).abs()).collect();
    abs_deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mad = percentile(&abs_deviations, 50.0)?;

    let normalized_mad = MAD_CONSISTENCY * mad;

    if normalized_mad < f64::EPSILON {
        return None;
    }

    Some(
        values
            .iter()
            .map(|value| MODIFIED_Z_SCALE * (value - median) / normalized_mad)
            .collect(),
    )
}

/// Value-space bounds at which `|score| == threshold` for the given method.
///
/// For [`Method::ZScore`] this is the σ-bounds formula ported verbatim from
/// `oxigeo-qc/src/raster/consistency.rs:441-442`
/// (`mean ± threshold·σ`, population σ). The IQR and modified-Z bounds
/// invert the respective score formulas around the median so that every
/// method reports the interval outside of which pixels are flagged.
fn detection_bounds(method: Method, valid: &[f64], threshold: f64) -> Option<(f64, f64)> {
    match method {
        Method::ZScore => {
            if valid.len() < 2 {
                return None;
            }
            let n = valid.len() as f64;
            let mean = valid.iter().sum::<f64>() / n;
            let variance = valid.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
            let std_dev = variance.sqrt();
            if std_dev < f64::EPSILON {
                return None;
            }
            // oxigeo-qc consistency.rs:441-442 verbatim.
            let lower_bound = mean - (threshold * std_dev);
            let upper_bound = mean + (threshold * std_dev);
            Some((lower_bound, upper_bound))
        }
        Method::Iqr => {
            if valid.len() < 4 {
                return None;
            }
            let mut sorted = valid.to_vec();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let q1 = percentile(&sorted, 25.0)?;
            let q3 = percentile(&sorted, 75.0)?;
            let iqr = q3 - q1;
            if iqr < f64::EPSILON {
                return None;
            }
            let median = percentile(&sorted, 50.0)?;
            Some((median - threshold * iqr, median + threshold * iqr))
        }
        Method::ModifiedZ => {
            if valid.len() < 2 {
                return None;
            }
            let mut sorted = valid.to_vec();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let median = percentile(&sorted, 50.0)?;
            let mut abs_deviations: Vec<f64> = valid.iter().map(|x| (x - median).abs()).collect();
            abs_deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mad = percentile(&abs_deviations, 50.0)?;
            let normalized_mad = MAD_CONSISTENCY * mad;
            if normalized_mad < f64::EPSILON {
                return None;
            }
            let half_width = threshold * normalized_mad / MODIFIED_Z_SCALE;
            Some((median - half_width, median + half_width))
        }
    }
}

/// Per-pixel scores aligned with the input raster.
struct Scored {
    /// Score for every input pixel; NaN for nodata/non-finite pixels and for
    /// every pixel when the statistics are degenerate.
    scores_full: Vec<f32>,
    /// Full-precision scores of the valid pixels (parallel to
    /// `valid_indices`); empty when degenerate. The threshold rule is
    /// applied to these `f64` values, exactly like the native `detect`.
    scores_valid: Vec<f64>,
    /// Indices of valid pixels (parallel to `valid` / `scores_valid`).
    valid_indices: Vec<usize>,
    /// Valid samples promoted to `f64`.
    valid: Vec<f64>,
    /// True when the native detector would have errored (insufficient data
    /// or near-zero spread); no scores exist in that case.
    degenerate: bool,
}

/// Filters invalid samples and computes per-pixel scores for `method`.
fn score_raster(method: Method, values: &[f32], nodata: f32) -> Scored {
    let mut valid_indices = Vec::new();
    let mut valid = Vec::new();
    for (i, &v) in values.iter().enumerate() {
        if is_valid(v, nodata) {
            valid_indices.push(i);
            valid.push(f64::from(v));
        }
    }

    let computed = match method {
        Method::ZScore => zscore_scores(&valid),
        Method::Iqr => iqr_scores(&valid),
        Method::ModifiedZ => modified_zscore_scores(&valid),
    };

    let mut scores_full = vec![f32::NAN; values.len()];
    let (scores_valid, degenerate) = match computed {
        Some(scores) => {
            for (k, &score) in scores.iter().enumerate() {
                scores_full[valid_indices[k]] = score as f32;
            }
            (scores, false)
        }
        None => (Vec::new(), true),
    };

    Scored {
        scores_full,
        scores_valid,
        valid_indices,
        valid,
        degenerate,
    }
}

/// Full detection result for one method/threshold combination.
struct Detection {
    /// Per-pixel flag: 1 = anomalous, 0 = normal or invalid (nodata/NaN).
    mask: Vec<u8>,
    /// Number of valid (finite, non-nodata) pixels.
    valid_count: usize,
    /// Number of flagged pixels.
    anomaly_count: usize,
    /// True when statistics were degenerate (constant input etc.).
    degenerate: bool,
    /// Value-space `(lower, upper)` detection bounds, when defined.
    bounds: Option<(f64, f64)>,
}

/// Runs `method` over the raster and applies the analytics threshold rule
/// (`score.abs() >= threshold`, `detect` in
/// `oxigeo-analytics/src/timeseries/anomaly.rs:66-77`).
fn detect(method: Method, values: &[f32], threshold: f64, nodata: f32) -> Detection {
    let scored = score_raster(method, values, nodata);
    let mut mask = vec![0u8; values.len()];
    let mut anomaly_count = 0usize;

    if !scored.degenerate {
        for (k, &score) in scored.scores_valid.iter().enumerate() {
            // Use >= to include boundary cases where score equals threshold.
            if score.abs() >= threshold {
                mask[scored.valid_indices[k]] = 1;
                anomaly_count += 1;
            }
        }
    }

    let bounds = if scored.degenerate {
        None
    } else {
        detection_bounds(method, &scored.valid, threshold)
    };

    Detection {
        mask,
        valid_count: scored.valid.len(),
        anomaly_count,
        degenerate: scored.degenerate,
        bounds,
    }
}

/// Builds the summary JSON for a detection run.
///
/// The percentage follows the `oxigeo-qc` outlier-percentage rule
/// (`consistency.rs:458-462`): `anomaly_count / valid_count · 100`, zero when
/// there are no valid pixels.
fn summary_value(method: Method, threshold: f64, total_count: usize, det: &Detection) -> String {
    let anomaly_pct = if det.valid_count > 0 {
        (det.anomaly_count as f64 / det.valid_count as f64) * 100.0
    } else {
        0.0
    };
    let (lower, upper) = match det.bounds {
        Some((lo, hi)) => (Some(lo), Some(hi)),
        None => (None, None),
    };
    serde_json::json!({
        "method": method.canonical_name(),
        "threshold": threshold,
        "total_count": total_count,
        "valid_count": det.valid_count,
        "anomaly_count": det.anomaly_count,
        "anomaly_pct": anomaly_pct,
        "lower_bound": lower,
        "upper_bound": upper,
        "degenerate": det.degenerate,
    })
    .to_string()
}

/// Anomaly-detection kernels for `f32` rasters, exposed to JavaScript.
///
/// All operations are stateless class methods: callers pass the raster
/// (`values`), a detection `threshold`, and the `nodata` sentinel (use `NaN`
/// when the raster has none). Masks and scores are index-aligned with the
/// input, so they can be reshaped to `width × height` on the JS side.
///
/// Numeric parity: see the module documentation — the kernels are verbatim
/// ports from `oxigeo-analytics` (Z-score / IQR / modified Z-score /
/// percentile) and `oxigeo-qc` (σ-bounds, percentage rule).
#[wasm_bindgen]
pub struct WasmAnomaly;

#[wasm_bindgen]
impl WasmAnomaly {
    /// Z-score anomaly mask: 1 where `|z| >= threshold`, else 0.
    ///
    /// Degenerate input (fewer than 2 valid samples, or σ ≈ 0 as for a
    /// constant raster) yields an all-zero mask — nothing is flagged and
    /// nothing panics. Typical threshold: 3.0.
    #[wasm_bindgen(js_name = zscoreMask)]
    pub fn zscore_mask(values: &[f32], threshold: f64, nodata: f32) -> Vec<u8> {
        detect(Method::ZScore, values, threshold, nodata).mask
    }

    /// IQR anomaly mask: 1 where `|x − median| / iqr >= threshold`, else 0.
    ///
    /// Degenerate input (fewer than 4 valid samples, or IQR ≈ 0) yields an
    /// all-zero mask. Typical threshold: 1.5.
    #[wasm_bindgen(js_name = iqrMask)]
    pub fn iqr_mask(values: &[f32], threshold: f64, nodata: f32) -> Vec<u8> {
        detect(Method::Iqr, values, threshold, nodata).mask
    }

    /// Modified Z-score anomaly mask (MAD-based): 1 where
    /// `|0.6745·(x − median) / (1.4826·MAD)| >= threshold`, else 0.
    ///
    /// Degenerate input (fewer than 2 valid samples, or MAD ≈ 0) yields an
    /// all-zero mask. Typical threshold: 3.5.
    #[wasm_bindgen(js_name = modifiedZscoreMask)]
    pub fn modified_zscore_mask(values: &[f32], threshold: f64, nodata: f32) -> Vec<u8> {
        detect(Method::ModifiedZ, values, threshold, nodata).mask
    }

    /// Per-pixel anomaly scores for `method` (`"zscore"`, `"iqr"`, or
    /// `"modified_zscore"`), index-aligned with `values`.
    ///
    /// Invalid pixels (NaN / non-finite / nodata) score NaN; degenerate
    /// statistics yield all-NaN scores; an unknown method name yields an
    /// empty vector (no exception, mirroring the `WasmTerrain` wrappers).
    #[wasm_bindgen]
    pub fn scores(method: &str, values: &[f32], nodata: f32) -> Vec<f32> {
        match Method::parse(method) {
            Some(m) => score_raster(m, values, nodata).scores_full,
            None => Vec::new(),
        }
    }

    /// Renders a mask as RGBA [`ImageData`]: flagged pixels get
    /// `(r, g, b, a)`, all other pixels are fully transparent — ready to be
    /// drawn over the base map as an anomaly overlay.
    ///
    /// Errors if `mask.len() != width * height` or a dimension is zero.
    #[wasm_bindgen(js_name = maskToImageData)]
    pub fn mask_to_image_data(
        mask: &[u8],
        width: u32,
        height: u32,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    ) -> Result<ImageData, JsValue> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| JsValue::from_str("mask dimensions overflow"))?;
        if width == 0 || height == 0 {
            return Err(JsValue::from_str("mask dimensions must be non-zero"));
        }
        if mask.len() != expected {
            return Err(JsValue::from_str(&format!(
                "mask length {} does not match {width}x{height}",
                mask.len()
            )));
        }

        let mut rgba = vec![0u8; expected * 4];
        for (i, &m) in mask.iter().enumerate() {
            if m != 0 {
                rgba[i * 4] = r;
                rgba[i * 4 + 1] = g;
                rgba[i * 4 + 2] = b;
                rgba[i * 4 + 3] = a;
            }
        }
        let clamped = wasm_bindgen::Clamped(rgba.as_slice());
        ImageData::new_with_u8_clamped_array_and_sh(clamped, width, height)
    }

    /// Runs detection and returns a JSON summary suitable for the GeoVault
    /// session ledger (`anomaly.detect` operation params):
    /// `method`, `threshold`, `total_count`, `valid_count`, `anomaly_count`,
    /// `anomaly_pct` (per the `oxigeo-qc` percentage rule), value-space
    /// `lower_bound` / `upper_bound` (σ-bounds for `zscore`, null when
    /// degenerate), and a `degenerate` flag.
    ///
    /// An unknown method name returns `{"error": "..."}` instead of throwing.
    #[wasm_bindgen(js_name = summaryJson)]
    pub fn summary_json(method: &str, values: &[f32], threshold: f64, nodata: f32) -> String {
        match Method::parse(method) {
            Some(m) => {
                let det = detect(m, values, threshold, nodata);
                summary_value(m, threshold, values.len(), &det)
            }
            None => serde_json::json!({
                "error": format!(
                    "unknown anomaly method '{method}'; expected zscore | iqr | modified_zscore"
                ),
            })
            .to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Analytics `test_z_score_detection` parity: `[1, 2, 3, 4, 100]` at
    /// threshold 1.9 (the outlier's z is ≈ 1.99934, just under 2.0 because it
    /// inflates both mean and σ) flags exactly index 4.
    #[test]
    fn zscore_parity_analytics_1_9_threshold() {
        let values = [1.0f32, 2.0, 3.0, 4.0, 100.0];
        let mask = WasmAnomaly::zscore_mask(&values, 1.9, f32::NAN);
        assert_eq!(mask, vec![0, 0, 0, 0, 1]);

        // Hand-computed: mean 22, population variance 1522, z4 = 78/√1522.
        let scores = WasmAnomaly::scores("zscore", &values, f32::NAN);
        assert_eq!(scores.len(), 5);
        let expected_z4 = 78.0f64 / 1522.0f64.sqrt();
        assert!((f64::from(scores[4]) - expected_z4).abs() < 1e-6);
        let expected_z0 = -21.0f64 / 1522.0f64.sqrt();
        assert!((f64::from(scores[0]) - expected_z0).abs() < 1e-6);
    }

    /// Analytics `test_no_anomalies` parity: `[1..=5]` at threshold 3.0 flags
    /// nothing.
    #[test]
    fn zscore_no_anomalies_at_3_sigma() {
        let values = [1.0f32, 2.0, 3.0, 4.0, 5.0];
        let mask = WasmAnomaly::zscore_mask(&values, 3.0, f32::NAN);
        assert!(mask.iter().all(|&m| m == 0));
    }

    /// Analytics `test_iqr_detection` parity: `[1, 2, 3, 4, 5, 100]` at
    /// threshold 1.5 flags exactly index 5. Hand-computed: q1 2.25, q3 4.75,
    /// iqr 2.5, median 3.5 → scores `[1.0, 0.6, 0.2, 0.2, 0.6, 38.6]`.
    #[test]
    fn iqr_parity_analytics_case() {
        let values = [1.0f32, 2.0, 3.0, 4.0, 5.0, 100.0];
        let mask = WasmAnomaly::iqr_mask(&values, 1.5, f32::NAN);
        assert_eq!(mask, vec![0, 0, 0, 0, 0, 1]);

        let scores = WasmAnomaly::scores("iqr", &values, f32::NAN);
        let expected = [1.0f64, 0.6, 0.2, 0.2, 0.6, 38.6];
        for (got, want) in scores.iter().zip(expected.iter()) {
            // Scores are narrowed to f32 for JS, so compare with a relative
            // tolerance of a few f32 ULPs.
            let tolerance = want.abs().max(1.0) * 1e-6;
            assert!(
                (f64::from(*got) - want).abs() < tolerance,
                "iqr score {got} != {want}"
            );
        }
    }

    /// Analytics `test_modified_z_score` parity: `[1, 2, 3, 4, 5, 100]` at
    /// threshold 3.5 flags index 5. Hand-computed: median 3.5, MAD 1.5,
    /// normalized MAD 2.2239 → score5 = 0.6745·96.5/2.2239 ≈ 29.268.
    #[test]
    fn modified_zscore_parity_analytics_case() {
        let values = [1.0f32, 2.0, 3.0, 4.0, 5.0, 100.0];
        let mask = WasmAnomaly::modified_zscore_mask(&values, 3.5, f32::NAN);
        assert_eq!(mask, vec![0, 0, 0, 0, 0, 1]);

        let scores = WasmAnomaly::scores("modified_zscore", &values, f32::NAN);
        let expected_s5 = 0.6745 * 96.5 / (1.4826 * 1.5);
        assert!((f64::from(scores[5]) - expected_s5).abs() < 1e-4);
        let expected_s0 = 0.6745 * (1.0 - 3.5) / (1.4826 * 1.5);
        assert!((f64::from(scores[0]) - expected_s0).abs() < 1e-6);
    }

    /// Analytics `test_percentile` parity plus edge cases carried over from
    /// the source error paths.
    #[test]
    fn percentile_parity_and_edges() {
        let data = [1.0f64, 2.0, 3.0, 4.0, 5.0];
        assert!((percentile(&data, 50.0).expect("p50") - 3.0).abs() < 1e-10);
        assert!((percentile(&data, 25.0).expect("p25") - 2.0).abs() < 1e-10);
        assert!((percentile(&data, 75.0).expect("p75") - 4.0).abs() < 1e-10);
        // Single element short-circuits.
        assert!((percentile(&[7.5], 99.0).expect("single") - 7.5).abs() < 1e-12);
        // Source error paths → None.
        assert!(percentile(&[], 50.0).is_none());
        assert!(percentile(&data, -1.0).is_none());
        assert!(percentile(&data, 100.5).is_none());
    }

    /// Constant rasters are degenerate for every method: the mask flags
    /// nothing, scores are NaN, and nothing panics (the native detectors
    /// would return `numerical_instability` errors here).
    #[test]
    fn constant_input_yields_empty_mask_never_panics() {
        let values = [5.0f32; 16];
        for method in ["zscore", "iqr", "modified_zscore"] {
            let scores = WasmAnomaly::scores(method, &values, f32::NAN);
            assert_eq!(scores.len(), 16, "{method} scores keep raster length");
            assert!(scores.iter().all(|s| s.is_nan()), "{method} scores NaN");
        }
        assert!(
            WasmAnomaly::zscore_mask(&values, 3.0, f32::NAN)
                .iter()
                .all(|&m| m == 0)
        );
        assert!(
            WasmAnomaly::iqr_mask(&values, 1.5, f32::NAN)
                .iter()
                .all(|&m| m == 0)
        );
        assert!(
            WasmAnomaly::modified_zscore_mask(&values, 3.5, f32::NAN)
                .iter()
                .all(|&m| m == 0)
        );
    }

    /// NaN, infinities, and the nodata sentinel are filtered before any
    /// statistic; the surviving samples reproduce the analytics case and the
    /// invalid pixels stay unflagged with NaN scores.
    #[test]
    fn nan_and_nodata_filtered_before_statistics() {
        let nodata = -9999.0f32;
        let values = [
            1.0f32,
            2.0,
            f32::NAN,
            3.0,
            nodata,
            4.0,
            f32::INFINITY,
            100.0,
        ];
        // Valid subset is [1, 2, 3, 4, 100] → same as the 1.9-threshold case.
        let mask = WasmAnomaly::zscore_mask(&values, 1.9, nodata);
        assert_eq!(mask, vec![0, 0, 0, 0, 0, 0, 0, 1]);

        let scores = WasmAnomaly::scores("zscore", &values, nodata);
        assert!(scores[2].is_nan(), "NaN pixel scores NaN");
        assert!(scores[4].is_nan(), "nodata pixel scores NaN");
        assert!(scores[6].is_nan(), "infinite pixel scores NaN");
        let expected_z = 78.0f64 / 1522.0f64.sqrt();
        assert!((f64::from(scores[7]) - expected_z).abs() < 1e-6);
    }

    /// σ-bounds parity with `oxigeo-qc` `detect_outliers`
    /// (consistency.rs:441-442): `[2,4,4,4,5,5,7,9]` has mean 5 and
    /// population σ 2, so k = 2 gives bounds (1, 9); the sample at 9 sits
    /// exactly on the boundary and IS flagged (`>=` rule), giving 1/8 =
    /// 12.5 %.
    #[test]
    fn sigma_bounds_qc_parity_in_summary() {
        let values = [2.0f32, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let json = WasmAnomaly::summary_json("zscore", &values, 2.0, f32::NAN);
        let v: serde_json::Value = serde_json::from_str(&json).expect("summary parses");

        assert_eq!(v["method"], "zscore");
        assert_eq!(v["total_count"], 8);
        assert_eq!(v["valid_count"], 8);
        assert_eq!(v["anomaly_count"], 1, "boundary score 2.0 flagged by >=");
        let lower = v["lower_bound"].as_f64().expect("lower bound");
        let upper = v["upper_bound"].as_f64().expect("upper bound");
        assert!((lower - 1.0).abs() < 1e-12, "lower = mean - k*sigma = 1");
        assert!((upper - 9.0).abs() < 1e-12, "upper = mean + k*sigma = 9");
        let pct = v["anomaly_pct"].as_f64().expect("pct");
        assert!((pct - 12.5).abs() < 1e-12);
        assert_eq!(v["degenerate"], false);

        // The mask agrees with the summary counts.
        let mask = WasmAnomaly::zscore_mask(&values, 2.0, f32::NAN);
        assert_eq!(mask.iter().map(|&m| usize::from(m)).sum::<usize>(), 1);
        assert_eq!(mask[7], 1);
    }

    /// Degenerate summaries report null bounds, zero counts, and the flag —
    /// including the fully-empty and all-nodata rasters.
    #[test]
    fn degenerate_summary_reports_null_bounds() {
        let constant = [3.0f32; 8];
        let json = WasmAnomaly::summary_json("iqr", &constant, 1.5, f32::NAN);
        let v: serde_json::Value = serde_json::from_str(&json).expect("summary parses");
        assert_eq!(v["degenerate"], true);
        assert_eq!(v["anomaly_count"], 0);
        assert!(v["lower_bound"].is_null());
        assert!(v["upper_bound"].is_null());

        let empty: [f32; 0] = [];
        let json = WasmAnomaly::summary_json("zscore", &empty, 3.0, f32::NAN);
        let v: serde_json::Value = serde_json::from_str(&json).expect("summary parses");
        assert_eq!(v["valid_count"], 0);
        assert_eq!(v["anomaly_pct"], 0.0, "qc rule: pct 0 when no valid");

        let all_nodata = [-9999.0f32; 4];
        let json = WasmAnomaly::summary_json("modified_zscore", &all_nodata, 3.5, -9999.0);
        let v: serde_json::Value = serde_json::from_str(&json).expect("summary parses");
        assert_eq!(v["valid_count"], 0);
        assert_eq!(v["degenerate"], true);
    }

    /// Sample-count minimums mirror the analytics `insufficient_data` errors:
    /// Z-score/modified-Z need 2 samples, IQR needs 4.
    #[test]
    fn insufficient_samples_degrade_gracefully() {
        let one = [42.0f32];
        assert_eq!(WasmAnomaly::zscore_mask(&one, 1.0, f32::NAN), vec![0]);
        assert_eq!(
            WasmAnomaly::modified_zscore_mask(&one, 1.0, f32::NAN),
            vec![0]
        );
        let three = [1.0f32, 2.0, 300.0];
        assert_eq!(
            WasmAnomaly::iqr_mask(&three, 0.1, f32::NAN),
            vec![0, 0, 0],
            "IQR needs at least 4 valid samples"
        );
        // Empty input → empty (zero-length) outputs, no panic.
        let empty: [f32; 0] = [];
        assert!(WasmAnomaly::zscore_mask(&empty, 3.0, f32::NAN).is_empty());
        assert!(WasmAnomaly::scores("zscore", &empty, f32::NAN).is_empty());
    }

    /// Method-name parsing: canonical names, tolerated aliases, and the
    /// empty-output / error-JSON fallbacks for unknown names.
    #[test]
    fn method_parsing_and_unknown_method_fallbacks() {
        assert_eq!(Method::parse("zscore"), Some(Method::ZScore));
        assert_eq!(Method::parse("Z-Score"), Some(Method::ZScore));
        assert_eq!(Method::parse("IQR"), Some(Method::Iqr));
        assert_eq!(Method::parse("modified_zscore"), Some(Method::ModifiedZ));
        assert_eq!(Method::parse("modified-zscore"), Some(Method::ModifiedZ));
        assert_eq!(Method::parse("grubbs"), None);

        let values = [1.0f32, 2.0, 3.0];
        assert!(WasmAnomaly::scores("grubbs", &values, f32::NAN).is_empty());
        let json = WasmAnomaly::summary_json("grubbs", &values, 3.0, f32::NAN);
        let v: serde_json::Value = serde_json::from_str(&json).expect("error json parses");
        assert!(
            v["error"]
                .as_str()
                .expect("error message")
                .contains("grubbs")
        );
    }

    /// IQR / modified-Z detection bounds invert their score formulas: a value
    /// placed exactly on the bound scores exactly the threshold.
    #[test]
    fn detection_bounds_invert_score_formulas() {
        let valid = [1.0f64, 2.0, 3.0, 4.0, 5.0, 100.0];

        let (lo, hi) = detection_bounds(Method::Iqr, &valid, 1.5).expect("iqr bounds");
        // median 3.5, iqr 2.5 → 3.5 ± 3.75.
        assert!((lo - (3.5 - 3.75)).abs() < 1e-12);
        assert!((hi - (3.5 + 3.75)).abs() < 1e-12);
        let iqr_scores_at_bounds = iqr_scores(&valid).expect("iqr scores");
        // Score of a value at the upper bound equals the threshold.
        assert!(((hi - 3.5) / 2.5 - 1.5).abs() < 1e-12);
        assert!(!iqr_scores_at_bounds.is_empty());

        let (lo, hi) = detection_bounds(Method::ModifiedZ, &valid, 3.5).expect("modz bounds");
        let normalized_mad = 1.4826 * 1.5;
        let half = 3.5 * normalized_mad / 0.6745;
        assert!((lo - (3.5 - half)).abs() < 1e-9);
        assert!((hi - (3.5 + half)).abs() < 1e-9);
        // Inversion check: score at the bound == threshold.
        assert!((0.6745 * (hi - 3.5) / normalized_mad - 3.5).abs() < 1e-9);
    }

    /// The threshold comparison uses `>=` exactly as the analytics `detect`
    /// (a score equal to the threshold IS an anomaly).
    #[test]
    fn threshold_boundary_uses_greater_or_equal() {
        // [2,4,4,4,5,5,7,9]: z(9) = (9-5)/2 = 2.0 exactly.
        let values = [2.0f32, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let mask = WasmAnomaly::zscore_mask(&values, 2.0, f32::NAN);
        assert_eq!(mask[7], 1, "score == threshold must be flagged");
        // Just above the score, nothing is flagged.
        let mask = WasmAnomaly::zscore_mask(&values, 2.0 + 1e-9, f32::NAN);
        assert!(mask.iter().all(|&m| m == 0));
    }

    /// Score vectors always match the input length for known methods, with
    /// finite scores exactly on the valid pixels.
    #[test]
    fn scores_are_index_aligned() {
        let nodata = 0.0f32;
        let values = [nodata, 10.0, 12.0, 11.0, 13.0, 90.0, nodata];
        let scores = WasmAnomaly::scores("modified_zscore", &values, nodata);
        assert_eq!(scores.len(), values.len());
        assert!(scores[0].is_nan());
        assert!(scores[6].is_nan());
        for i in 1..=5 {
            assert!(scores[i].is_finite(), "valid pixel {i} has a score");
        }
        // The outlier at index 5 dominates.
        let max_idx = (0..values.len())
            .filter(|&i| scores[i].is_finite())
            .max_by(|&a, &b| {
                scores[a]
                    .abs()
                    .partial_cmp(&scores[b].abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("some finite score");
        assert_eq!(max_idx, 5);
    }
}

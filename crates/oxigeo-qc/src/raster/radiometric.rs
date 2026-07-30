//! Per-sensor radiometric range validation.
//!
//! Validates that pixel values in each band of a raster file fall within the
//! expected ranges for a known sensor profile (Landsat 8/9, Sentinel-2,
//! MODIS) or a user-supplied custom profile.
//!
//! # Algorithm
//!
//! Deterministic stride sampling is used (stride = `max(1, total_pixels / 10_000)`)
//! to avoid reading the entire raster into memory. For each band the following
//! statistics are computed from the sample:
//!
//! - min, max, mean
//! - approximate p99 (sort-based)
//! - out-of-range fraction (`oor_fraction`)
//!
//! Issues emitted:
//!
//! - **Critical**: `oor_fraction > critical_oor_threshold` (default 0.1 %)
//! - **Major**: any sample is OOR (`oor_fraction > 0`)
//! - **Warning**: sampled mean deviates from `expected_mean` by more than
//!   `mean_drift_sigma * expected_std`

use std::path::Path;

use oxigeo_core::io::FileDataSource;
use oxigeo_geotiff::GeoTiffReader;

use crate::error::{QcIssue, QcResult, Severity};
use crate::raster::band_scan::{RasterScan, native, scan_band};

// ── Band range ────────────────────────────────────────────────────────────────

/// Per-band expected value range for a sensor type.
#[derive(Debug, Clone)]
pub struct BandRange {
    /// Minimum valid pixel value (inclusive).
    pub min: f64,
    /// Maximum valid pixel value (inclusive).
    pub max: f64,
    /// Expected mean value for the band (optional, used for drift check).
    pub expected_mean: Option<f64>,
    /// Expected standard deviation for the band (optional, used for drift check).
    pub expected_std: Option<f64>,
}

// ── Sensor profile ────────────────────────────────────────────────────────────

/// Known sensor profiles with expected reflectance / DN ranges.
#[derive(Debug, Clone)]
pub enum SensorProfile {
    /// Landsat 8 Surface Reflectance (scaled by 10 000, range 0–10 000).
    Landsat8Sr,
    /// Landsat 9 Surface Reflectance (same scaling as L8 SR).
    Landsat9Sr,
    /// Sentinel-2 Level-2A (BOA reflectance scaled 0–10 000).
    Sentinel2L2a,
    /// Sentinel-2 Level-1C (TOA reflectance scaled 0–10 000).
    Sentinel2L1c,
    /// MODIS Surface Reflectance (range −100 to 16 000, with scale factor).
    ModisSr,
    /// Custom profile with per-band ranges (band index → `BandRange`).
    ///
    /// If the requested band index exceeds `ranges.len()`, a fallback range of
    /// `[0, 65535]` with no expected statistics is returned.
    Custom {
        /// Per-band expected value ranges; indexed by 0-based band index.
        ranges: Vec<BandRange>,
    },
}

impl SensorProfile {
    /// Returns the expected range for a given 0-based band index.
    #[must_use]
    pub fn band_range(&self, band_idx: usize) -> BandRange {
        match self {
            Self::Landsat8Sr | Self::Landsat9Sr => BandRange {
                min: 0.0,
                max: 10_000.0,
                expected_mean: Some(2_000.0),
                expected_std: Some(1_500.0),
            },
            Self::Sentinel2L2a | Self::Sentinel2L1c => BandRange {
                min: 0.0,
                max: 10_000.0,
                expected_mean: Some(2_500.0),
                expected_std: Some(2_000.0),
            },
            Self::ModisSr => BandRange {
                min: -100.0,
                max: 16_000.0,
                expected_mean: Some(3_000.0),
                expected_std: Some(2_500.0),
            },
            Self::Custom { ranges } => ranges.get(band_idx).cloned().unwrap_or(BandRange {
                min: 0.0,
                max: 65_535.0,
                expected_mean: None,
                expected_std: None,
            }),
        }
    }
}

// ── Per-band result ───────────────────────────────────────────────────────────

/// Statistics for a single band produced by the radiometric validator.
#[derive(Debug, Clone)]
pub struct BandRadiometricResult {
    /// 0-based band index.
    pub band_idx: usize,
    /// Minimum sampled pixel value.
    pub min_sampled: f64,
    /// Maximum sampled pixel value.
    pub max_sampled: f64,
    /// Mean of sampled pixel values.
    pub mean_sampled: f64,
    /// Approximate 99th-percentile of sampled pixel values.
    pub p99_sampled: f64,
    /// Fraction of sampled pixels that are out-of-range `[0.0, 1.0]`.
    pub oor_fraction: f64,
}

// ── Overall result ────────────────────────────────────────────────────────────

/// Overall radiometric validation result.
#[derive(Debug, Clone)]
pub struct RadiometricValidationResult {
    /// Issues raised during validation.
    pub issues: Vec<QcIssue>,
    /// Per-band statistics.
    pub per_band: Vec<BandRadiometricResult>,
}

impl RadiometricValidationResult {
    /// Returns `true` if no `Major` or higher issues were raised.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.issues.iter().all(|i| i.severity < Severity::Major)
    }
}

// ── Validator ─────────────────────────────────────────────────────────────────

/// Per-sensor radiometric range validator.
///
/// Opens a GeoTIFF through the driver's band-aware read engine, samples pixels using a deterministic
/// stride, and emits [`crate::error::QcIssue`] entries when values fall outside
/// the profile's expected ranges.
#[derive(Debug, Clone)]
pub struct RadiometricValidator {
    /// Sensor profile (defines expected value ranges).
    pub profile: SensorProfile,
    /// Fraction of out-of-range samples that triggers a Critical issue.
    ///
    /// Default: `0.001` (0.1 %).
    pub critical_oor_threshold: f64,
    /// Mean drift threshold in multiples of `expected_std`.
    ///
    /// A Warning is emitted when
    /// `|sampled_mean - expected_mean| > mean_drift_sigma * expected_std`.
    /// Default: `2.0`.
    pub mean_drift_sigma: f64,
}

impl RadiometricValidator {
    /// Constructs a validator with default thresholds for the given profile.
    #[must_use]
    pub const fn new(profile: SensorProfile) -> Self {
        Self {
            profile,
            critical_oor_threshold: 0.001,
            mean_drift_sigma: 2.0,
        }
    }
}

impl Default for RadiometricValidator {
    fn default() -> Self {
        Self::new(SensorProfile::Sentinel2L2a)
    }
}

impl RadiometricValidator {
    /// Validates the radiometric content of a raster file.
    ///
    /// Uses deterministic stride sampling (~10 000 samples per band maximum).
    ///
    /// The sample grid is identical for chunky (`PlanarConfiguration = 1`) and
    /// planar (`= 2`) files: samples come from the driver's band-aware read
    /// engine (see `band_scan`), not from a hand-de-interleaved
    /// `read_tile`, which used to reach only ~`1/spp` of a planar file and
    /// attribute those samples to the wrong bands.
    pub fn check_file<P: AsRef<Path>>(&self, path: P) -> QcResult<RadiometricValidationResult> {
        let source = FileDataSource::open(path.as_ref()).map_err(|e| {
            crate::error::QcError::RasterError(format!("Failed to open raster: {}", e))
        })?;
        let reader = GeoTiffReader::open(source).map_err(|e| {
            crate::error::QcError::RasterError(format!("Failed to read GeoTIFF: {}", e))
        })?;
        let scan = RasterScan::probe(&reader)?;
        let band_count = scan.band_count;

        let mut issues = Vec::new();
        let mut per_band = Vec::with_capacity(band_count);

        for band_idx in 0..band_count {
            let samples = sample_band(&reader, &scan, band_idx)?;
            if samples.is_empty() {
                continue;
            }

            let range = self.profile.band_range(band_idx);
            let band_result = compute_band_stats(band_idx, &samples, &range);

            emit_issues(
                &mut issues,
                &band_result,
                &range,
                band_idx,
                self.critical_oor_threshold,
                self.mean_drift_sigma,
            );
            per_band.push(band_result);
        }

        Ok(RadiometricValidationResult { issues, per_band })
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Reads pixel values for one band using stride-based deterministic sampling.
///
/// Stride = `max(1, total_pixels / 10_000)` over the band's row-major pixel
/// index, which is exactly the grid the old tile walk produced for a chunky
/// file — and, unlike it, the same grid for a planar one.
fn sample_band<S: oxigeo_core::io::DataSource>(
    reader: &GeoTiffReader<S>,
    scan: &RasterScan,
    band_idx: usize,
) -> QcResult<Vec<f64>> {
    let total_pixels = usize::try_from(scan.total_pixels()).map_err(|_| {
        crate::error::QcError::RasterError("raster pixel count overflows usize".to_string())
    })?;
    if total_pixels == 0 {
        return Ok(Vec::new());
    }

    let stride = total_pixels.div_ceil(10_000).max(1);

    // Pre-allocate a generous upper bound (total_pixels / stride + 1).
    let mut samples = Vec::with_capacity(total_pixels / stride + 1);

    scan_band(reader, scan, band_idx, |first_row, bytes| {
        let base = usize::try_from(first_row * scan.width).map_err(|_| {
            crate::error::QcError::RasterError("row offset overflows usize".to_string())
        })?;
        for (offset, sample) in bytes.chunks_exact(scan.bytes_per_sample).enumerate() {
            if !(base + offset).is_multiple_of(stride) {
                continue;
            }
            if let Some(v) = bytes_to_f64(sample, scan.data_type, scan.sample_format) {
                samples.push(v);
            }
        }
        Ok(())
    })?;

    Ok(samples)
}

/// Converts raw sample bytes to `f64` for any supported data type.
///
/// `bytes` is in the **host's** byte order: the driver normalises decoded samples
/// once, on the way out of block decode, so an `MM` file and its `II` twin
/// deliver identical bytes here. Consulting the file's byte order — which this
/// did before cool-japan/oxigeo#14, correctly at the time — would now swap an
/// `MM` file's samples a second time and turn every min/max/mean/out-of-range
/// verdict back into fiction.
fn bytes_to_f64(
    bytes: &[u8],
    dtype: oxigeo_core::types::RasterDataType,
    fmt: oxigeo_geotiff::tiff::SampleFormat,
) -> Option<f64> {
    use oxigeo_core::types::RasterDataType as DT;
    use oxigeo_geotiff::tiff::SampleFormat as SF;

    match (fmt, dtype) {
        (SF::UnsignedInteger, DT::UInt8) => bytes.first().map(|&v| v as f64),
        (SF::UnsignedInteger, DT::UInt16) => native::read_u16(bytes).map(|v| v as f64),
        (SF::UnsignedInteger, DT::UInt32) => native::read_u32(bytes).map(|v| v as f64),
        (SF::UnsignedInteger, DT::UInt64) => native::read_u64(bytes).map(|v| v as f64),
        (SF::SignedInteger, DT::Int8) => bytes.first().map(|&v| (v as i8) as f64),
        (SF::SignedInteger, DT::Int16) => native::read_i16(bytes).map(|v| v as f64),
        (SF::SignedInteger, DT::Int32) => native::read_i32(bytes).map(|v| v as f64),
        (SF::SignedInteger, DT::Int64) => native::read_i64(bytes).map(|v| v as f64),
        (SF::IeeeFloatingPoint, DT::Float32) => native::read_f32(bytes)
            .filter(|v| !v.is_nan())
            .map(f64::from),
        (SF::IeeeFloatingPoint, DT::Float64) => native::read_f64(bytes).filter(|v| !v.is_nan()),
        _ => None,
    }
}

/// Computes per-band statistics from raw samples.
fn compute_band_stats(
    band_idx: usize,
    samples: &[f64],
    range: &BandRange,
) -> BandRadiometricResult {
    debug_assert!(!samples.is_empty());

    let n = samples.len() as f64;
    let mut min = f64::MAX;
    let mut max = f64::MIN;
    let mut sum = 0.0_f64;
    let mut oor_count = 0usize;

    for &v in samples {
        if v < min {
            min = v;
        }
        if v > max {
            max = v;
        }
        sum += v;
        if v < range.min || v > range.max {
            oor_count += 1;
        }
    }

    let mean_sampled = sum / n;
    let oor_fraction = oor_count as f64 / samples.len() as f64;

    // Approximate p99: sort a clone and take the 99th-percentile index.
    let mut sorted = samples.to_vec();
    // Use partial_cmp to handle any residual NaN-free floats robustly.
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p99_idx = ((sorted.len() as f64 * 0.99) as usize).min(sorted.len().saturating_sub(1));
    let p99_sampled = sorted[p99_idx];

    BandRadiometricResult {
        band_idx,
        min_sampled: min,
        max_sampled: max,
        mean_sampled,
        p99_sampled,
        oor_fraction,
    }
}

/// Emits QC issues based on band statistics and thresholds.
fn emit_issues(
    issues: &mut Vec<QcIssue>,
    result: &BandRadiometricResult,
    range: &BandRange,
    band_idx: usize,
    critical_oor_threshold: f64,
    mean_drift_sigma: f64,
) {
    let band_label = band_idx + 1;

    if result.oor_fraction > critical_oor_threshold {
        issues.push(
            QcIssue::new(
                Severity::Critical,
                "radiometric",
                "High out-of-range fraction",
                format!(
                    "Band {}: {:.2}% of sampled pixels are outside the expected range \
                     [{}, {}] (threshold {:.1}%)",
                    band_label,
                    result.oor_fraction * 100.0,
                    range.min,
                    range.max,
                    critical_oor_threshold * 100.0,
                ),
            )
            .with_rule_id("RADIO-OOR-CRITICAL")
            .with_suggestion(
                "Check sensor calibration, apply atmospheric correction, \
                 or verify the correct sensor profile is selected.",
            ),
        );
    } else if result.oor_fraction > 0.0 {
        issues.push(
            QcIssue::new(
                Severity::Major,
                "radiometric",
                "Out-of-range pixels detected",
                format!(
                    "Band {}: {:.4}% of sampled pixels fall outside [{}, {}]",
                    band_label,
                    result.oor_fraction * 100.0,
                    range.min,
                    range.max,
                ),
            )
            .with_rule_id("RADIO-OOR-MAJOR"),
        );
    }

    // Mean drift check (only when both expected_mean and expected_std are set).
    if let (Some(exp_mean), Some(exp_std)) = (range.expected_mean, range.expected_std)
        && exp_std > 0.0
    {
        let drift = (result.mean_sampled - exp_mean).abs();
        if drift > mean_drift_sigma * exp_std {
            issues.push(
                QcIssue::new(
                    Severity::Warning,
                    "radiometric",
                    "Mean value drift detected",
                    format!(
                        "Band {}: sampled mean {:.1} deviates from expected mean {:.1} \
                             by {:.1} (threshold {:.1}× std = {:.1})",
                        band_label,
                        result.mean_sampled,
                        exp_mean,
                        drift,
                        mean_drift_sigma,
                        mean_drift_sigma * exp_std,
                    ),
                )
                .with_rule_id("RADIO-MEAN-DRIFT")
                .with_suggestion(
                    "Consider re-running atmospheric correction or verifying \
                         the radiometric calibration of the sensor.",
                ),
            );
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensor_profile_ranges_landsat8() {
        let r = SensorProfile::Landsat8Sr.band_range(0);
        assert_eq!(r.min, 0.0);
        assert_eq!(r.max, 10_000.0);
        assert_eq!(r.expected_mean, Some(2_000.0));
        assert_eq!(r.expected_std, Some(1_500.0));
    }

    #[test]
    fn test_sensor_profile_ranges_sentinel2_l2a() {
        let r = SensorProfile::Sentinel2L2a.band_range(2);
        assert_eq!(r.min, 0.0);
        assert_eq!(r.max, 10_000.0);
        assert_eq!(r.expected_mean, Some(2_500.0));
    }

    #[test]
    fn test_sensor_profile_ranges_modis() {
        let r = SensorProfile::ModisSr.band_range(0);
        assert_eq!(r.min, -100.0);
        assert_eq!(r.max, 16_000.0);
    }

    #[test]
    fn test_custom_profile_returns_correct_range() {
        let profile = SensorProfile::Custom {
            ranges: vec![
                BandRange {
                    min: 100.0,
                    max: 200.0,
                    expected_mean: Some(150.0),
                    expected_std: Some(10.0),
                },
                BandRange {
                    min: 50.0,
                    max: 300.0,
                    expected_mean: None,
                    expected_std: None,
                },
            ],
        };
        let r0 = profile.band_range(0);
        assert_eq!(r0.min, 100.0);
        assert_eq!(r0.max, 200.0);
        let r1 = profile.band_range(1);
        assert_eq!(r1.max, 300.0);
    }

    #[test]
    fn test_custom_profile_fallback_on_missing_band() {
        let profile = SensorProfile::Custom { ranges: vec![] };
        let r = profile.band_range(5);
        assert_eq!(r.min, 0.0);
        assert_eq!(r.max, 65_535.0);
        assert!(r.expected_mean.is_none());
    }

    #[test]
    fn test_validator_default_thresholds() {
        let v = RadiometricValidator::default();
        assert_eq!(v.critical_oor_threshold, 0.001);
        assert_eq!(v.mean_drift_sigma, 2.0);
    }

    #[test]
    fn test_oor_fraction_critical_threshold() {
        let range = BandRange {
            min: 0.0,
            max: 100.0,
            expected_mean: None,
            expected_std: None,
        };
        // 5 out-of-range samples out of 100 total → oor_fraction = 0.05 > 0.001 → Critical
        let samples: Vec<f64> = (0..95)
            .map(|i| i as f64)
            .chain([200.0, 200.0, 200.0, 200.0, 200.0])
            .collect();
        let band_result = compute_band_stats(0, &samples, &range);
        assert!((band_result.oor_fraction - 0.05).abs() < 1e-9);

        let mut issues = Vec::new();
        emit_issues(&mut issues, &band_result, &range, 0, 0.001, 2.0);
        assert!(
            issues.iter().any(|i| i.severity == Severity::Critical
                && i.rule_id.as_deref() == Some("RADIO-OOR-CRITICAL")),
            "expected Critical issue, got: {:#?}",
            issues
        );
    }

    #[test]
    fn test_oor_fraction_major_threshold() {
        let range = BandRange {
            min: 0.0,
            max: 100.0,
            expected_mean: None,
            expected_std: None,
        };
        // 1 out-of-range sample out of 1000 → oor_fraction = 0.001 which equals the threshold
        // → should still be Major (not Critical) because condition is strictly greater than.
        let mut samples: Vec<f64> = (0..999).map(|i| (i % 100) as f64).collect();
        samples.push(101.0); // OOR
        let band_result = compute_band_stats(0, &samples, &range);

        let mut issues = Vec::new();
        emit_issues(&mut issues, &band_result, &range, 0, 0.001, 2.0);
        // oor_fraction == critical_oor_threshold, not strictly greater → Major
        assert!(
            issues.iter().any(|i| i.severity == Severity::Major
                && i.rule_id.as_deref() == Some("RADIO-OOR-MAJOR")),
            "expected Major issue, got: {:#?}",
            issues
        );
    }

    #[test]
    fn test_mean_drift_warning() {
        let range = BandRange {
            min: 0.0,
            max: 10_000.0,
            expected_mean: Some(2_000.0),
            expected_std: Some(1_000.0),
        };
        // sampled mean ~ 8_000, drift = 6_000 >> 2.0 * 1_000 = 2_000 → Warning
        let samples: Vec<f64> = (0..100).map(|_| 8_000.0_f64).collect();
        let band_result = compute_band_stats(0, &samples, &range);
        let mut issues = Vec::new();
        emit_issues(&mut issues, &band_result, &range, 0, 0.001, 2.0);
        assert!(
            issues.iter().any(|i| i.severity == Severity::Warning
                && i.rule_id.as_deref() == Some("RADIO-MEAN-DRIFT")),
            "expected mean-drift Warning, got: {:#?}",
            issues
        );
    }

    #[test]
    fn test_no_issues_for_valid_samples() {
        let range = BandRange {
            min: 0.0,
            max: 10_000.0,
            expected_mean: Some(5_000.0),
            expected_std: Some(1_000.0),
        };
        // All samples in range and close to expected mean → no issues.
        let samples: Vec<f64> = (0..100).map(|i| 4_800.0 + (i as f64) * 4.0).collect();
        let band_result = compute_band_stats(0, &samples, &range);
        let mut issues = Vec::new();
        emit_issues(&mut issues, &band_result, &range, 0, 0.001, 2.0);
        assert!(issues.is_empty(), "unexpected issues: {:#?}", issues);
    }

    #[test]
    fn test_is_valid_no_major_issues() {
        let result = RadiometricValidationResult {
            issues: vec![QcIssue::new(
                Severity::Warning,
                "radiometric",
                "drift",
                "small drift",
            )],
            per_band: vec![],
        };
        assert!(
            result.is_valid(),
            "should be valid with only Warning issues"
        );
    }

    #[test]
    fn test_is_valid_with_major_issue() {
        let result = RadiometricValidationResult {
            issues: vec![QcIssue::new(
                Severity::Major,
                "radiometric",
                "OOR",
                "out of range",
            )],
            per_band: vec![],
        };
        assert!(!result.is_valid(), "should be invalid with Major issue");
    }
}

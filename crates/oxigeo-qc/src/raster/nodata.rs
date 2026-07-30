//! Multi-band NoData consistency validation.
//!
//! Walks every band of a raster and reports inconsistencies between the
//! declared NoData value (in metadata) and the NoData footprint actually
//! present in the pixel data:
//!
//! - `BandHasNoDataMetadataButNoNoDataPixels` (Warning): metadata claims a
//!   NoData sentinel but no pixel matches it.
//! - `BandHasNoDataPixelsButNoMetadata` (Major): pixels look like NoData
//!   sentinels (all 0 or all max-of-dtype, etc.) but no metadata declares
//!   them.
//! - `BandsHaveDifferentNoDataValues` (Major): multi-band raster with
//!   per-band NoData sentinels that disagree.
//! - `BandUnderCoversCommonFootprint` (Warning): a band's NoData mask is
//!   substantially smaller than the bitwise-AND across other bands' masks
//!   (likely fill-value pollution).
//!
//! Float comparison uses configurable epsilons (`1e-6` for f32, `1e-12` for
//! f64); IEEE-754 NaN is matched explicitly (NaN ≠ NaN normally, but we
//! treat any NaN pixel as matching a NaN sentinel).

use std::path::Path;

use oxigeo_core::io::FileDataSource;
use oxigeo_core::types::RasterDataType;
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_geotiff::tiff::SampleFormat;

use crate::error::{QcIssue, QcResult, Severity};
use crate::raster::band_scan::{RasterScan, native, scan_band};

/// Default float-comparison epsilon for `f32` bands.
pub const DEFAULT_FLOAT_EPS_F32: f32 = 1e-6;

/// Default float-comparison epsilon for `f64` bands.
pub const DEFAULT_FLOAT_EPS_F64: f64 = 1e-12;

/// Default outlier threshold (fraction): a band whose NoData coverage at the
/// common footprint is below `(1 - threshold)` of other bands' coverage gets
/// flagged.
pub const DEFAULT_OUTLIER_THRESHOLD: f64 = 0.5;

/// Validator for raster NoData consistency.
#[derive(Debug, Clone)]
pub struct NoDataValidator {
    /// Tolerance for matching declared float NoData values against pixels
    /// when the band data type is `Float32`.
    pub float_eps_f32: f32,
    /// Tolerance for matching declared float NoData values against pixels
    /// when the band data type is `Float64`.
    pub float_eps_f64: f64,
    /// Coverage outlier threshold (see module docs).
    pub outlier_threshold: f64,
}

impl Default for NoDataValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl NoDataValidator {
    /// Constructs a validator with default thresholds.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            float_eps_f32: DEFAULT_FLOAT_EPS_F32,
            float_eps_f64: DEFAULT_FLOAT_EPS_F64,
            outlier_threshold: DEFAULT_OUTLIER_THRESHOLD,
        }
    }

    /// Sets the f32 epsilon.
    #[must_use]
    pub const fn with_float_eps_f32(mut self, eps: f32) -> Self {
        self.float_eps_f32 = eps;
        self
    }

    /// Sets the f64 epsilon.
    #[must_use]
    pub const fn with_float_eps_f64(mut self, eps: f64) -> Self {
        self.float_eps_f64 = eps;
        self
    }

    /// Sets the outlier threshold.
    #[must_use]
    pub const fn with_outlier_threshold(mut self, t: f64) -> Self {
        self.outlier_threshold = t;
        self
    }

    /// Runs validation against a multi-band GeoTIFF file.
    ///
    /// Every pixel of every band is inspected, for chunky
    /// (`PlanarConfiguration = 1`) and planar (`= 2`) files alike: the samples
    /// come from the driver's band-aware read engine (see
    /// `band_scan`), which the older `read_tile`-based walk did
    /// not use and which is why a planar file used to be only ~`1/spp` scanned.
    ///
    /// One `bool` per pixel per band is retained for the common-footprint check,
    /// so peak memory is `width × height × band_count` bytes plus one bounded
    /// read stripe; the pixel data itself is streamed.
    pub fn check_file<P: AsRef<Path>>(&self, path: P) -> QcResult<NoDataValidationResult> {
        let source = FileDataSource::open(path.as_ref()).map_err(|e| {
            crate::error::QcError::RasterError(format!("Failed to open raster: {}", e))
        })?;
        let reader = GeoTiffReader::open(source).map_err(|e| {
            crate::error::QcError::RasterError(format!("Failed to read GeoTIFF: {}", e))
        })?;
        let scan = RasterScan::probe(&reader)?;
        let nodata_value = reader.nodata();

        // GDAL stores a single NoData per file; if extended per-band metadata
        // becomes available later, plug it in here. For now, every band shares
        // the same declared NoData (from GDAL_NODATA tag).
        let declared_per_band: Vec<Option<f64>> = (0..scan.band_count)
            .map(|_| nodata_value.as_f64())
            .collect();

        let masks = read_band_masks(&reader, &scan, &declared_per_band, self)?;
        Ok(self.evaluate_masks(masks, declared_per_band))
    }

    /// Evaluates per-band NoData masks against declared metadata, emitting
    /// issues. Internal helper exposed via `pub(crate)` for tests.
    pub(crate) fn evaluate_masks(
        &self,
        masks: Vec<NoDataBandMask>,
        declared: Vec<Option<f64>>,
    ) -> NoDataValidationResult {
        let mut issues = Vec::new();
        let mut per_band: Vec<NoDataBandStats> = Vec::with_capacity(masks.len());

        // BandsHaveDifferentNoDataValues: collect declared values that are present.
        let mut declared_values_present: Vec<f64> = declared.iter().flatten().copied().collect();
        declared_values_present
            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        declared_values_present.dedup_by(|a, b| (a.is_nan() && b.is_nan()) || a == b);
        if declared_values_present.len() > 1 {
            issues.push(
                QcIssue::new(
                    Severity::Major,
                    "nodata",
                    "Bands have different NoData values",
                    format!(
                        "Bands declare conflicting NoData sentinels: {:?}",
                        declared_values_present
                    ),
                )
                .with_rule_id("NODATA-VALUES-DIFFER")
                .with_suggestion(
                    "Reconcile band NoData values; multi-band rasters should share one sentinel.",
                ),
            );
        }

        // Per-band metadata-vs-pixel checks.
        for (band_idx, mask) in masks.iter().enumerate() {
            let declared_for_band = declared.get(band_idx).copied().flatten();
            let total = mask.total;
            let actual = mask.nodata_count;
            let coverage = if total > 0 {
                actual as f64 / total as f64
            } else {
                0.0
            };

            per_band.push(NoDataBandStats {
                band: (band_idx + 1) as u32,
                declared_nodata: declared_for_band,
                actual_nodata_count: actual,
                coverage_pct: coverage * 100.0,
            });

            match (declared_for_band, actual) {
                (Some(_), 0) => {
                    issues.push(
                        QcIssue::new(
                            Severity::Warning,
                            "nodata",
                            "Band has NoData metadata but no NoData pixels",
                            format!(
                                "Band {}: declared NoData but the raster contains zero matching pixels",
                                band_idx + 1
                            ),
                        )
                        .with_rule_id("NODATA-METADATA-WITHOUT-PIXELS"),
                    );
                }
                (None, n) if n > 0 && mask.suspected_unmarked_nodata => {
                    issues.push(
                        QcIssue::new(
                            Severity::Major,
                            "nodata",
                            "Band has NoData pixels but no metadata",
                            format!(
                                "Band {}: pixel data contains likely-NoData sentinels (count = {}) \
                                 but the file has no NoData metadata",
                                band_idx + 1,
                                n
                            ),
                        )
                        .with_rule_id("NODATA-PIXELS-WITHOUT-METADATA")
                        .with_suggestion("Add a GDAL_NODATA tag to declare the sentinel value."),
                    );
                }
                _ => {}
            }
        }

        // Common-footprint outlier check: take the bitwise-AND of NoData masks,
        // count how many cells are NoData in every band; for each band, count
        // its NoData pixels at those cells; if it falls under
        // (1 - outlier_threshold) of the common count, flag it.
        let mut common_count: u64 = 0;
        if let Some(first) = masks.first() {
            // Sum AND across bitmaps. We track a single counter rather than
            // materialising the full intersected bitmap.
            common_count = (0..first.bitmap.len())
                .filter(|i| {
                    masks
                        .iter()
                        .all(|m| m.bitmap.get(*i).copied().unwrap_or(false))
                })
                .count() as u64;
        }

        if masks.len() >= 2 {
            for (band_idx, mask) in masks.iter().enumerate() {
                let other_max = masks
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != band_idx)
                    .map(|(_, m)| m.nodata_count)
                    .max()
                    .unwrap_or(0);
                if other_max == 0 {
                    continue;
                }
                let ratio = mask.nodata_count as f64 / other_max as f64;
                if ratio < (1.0 - self.outlier_threshold) {
                    issues.push(
                        QcIssue::new(
                            Severity::Warning,
                            "nodata",
                            "Band under-covers common NoData footprint",
                            format!(
                                "Band {} has only {} NoData pixels vs other bands' max {} \
                                 (ratio {:.2}, threshold {:.2})",
                                band_idx + 1,
                                mask.nodata_count,
                                other_max,
                                ratio,
                                1.0 - self.outlier_threshold
                            ),
                        )
                        .with_rule_id("NODATA-COMMON-FOOTPRINT-OUTLIER")
                        .with_suggestion("Possible fill-value pollution; verify band consistency."),
                    );
                }
            }
        }

        NoDataValidationResult {
            issues,
            per_band,
            common_footprint_count: common_count,
        }
    }
}

/// Per-band statistics produced by [`NoDataValidator::check_file`].
#[derive(Debug, Clone)]
pub struct NoDataBandStats {
    /// 1-based band index.
    pub band: u32,
    /// Declared NoData sentinel for this band (or `None`).
    pub declared_nodata: Option<f64>,
    /// Count of pixels that match the declared NoData sentinel within ε.
    pub actual_nodata_count: u64,
    /// Percentage of pixels (0..=100) flagged as NoData in this band.
    pub coverage_pct: f64,
}

/// Result of NoData validation.
#[derive(Debug, Clone)]
pub struct NoDataValidationResult {
    /// Issues raised by the validator.
    pub issues: Vec<QcIssue>,
    /// Per-band statistics.
    pub per_band: Vec<NoDataBandStats>,
    /// Number of cells flagged as NoData in every single band.
    pub common_footprint_count: u64,
}

impl NoDataValidationResult {
    /// Returns `true` if no `Major` or higher issues were raised.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.issues.iter().any(|i| i.severity >= Severity::Major)
    }
}

/// Internal: per-band NoData mask + pixel count.
#[derive(Debug, Clone)]
pub(crate) struct NoDataBandMask {
    /// Bitmap: `bitmap[i] == true` iff pixel i is NoData.
    pub bitmap: Vec<bool>,
    /// Total pixel count.
    pub total: u64,
    /// Count of cells where `bitmap[i] == true`.
    pub nodata_count: u64,
    /// Heuristic: `true` if pixels that match a typical sentinel (0,
    /// max-of-dtype, NaN) appear without metadata declaring them.
    pub suspected_unmarked_nodata: bool,
}

/// Builds one NoData mask per band by streaming each band in full.
///
/// Reads go through [`scan_band`], so the walk is identical for tiled and
/// striped files and for chunky and planar storage; nothing here has to know the
/// block layout.
fn read_band_masks<S: oxigeo_core::io::DataSource>(
    reader: &GeoTiffReader<S>,
    scan: &RasterScan,
    declared_per_band: &[Option<f64>],
    validator: &NoDataValidator,
) -> QcResult<Vec<NoDataBandMask>> {
    let total_pixels = scan.total_pixels();
    let pixels = usize::try_from(total_pixels).map_err(|_| {
        crate::error::QcError::RasterError(format!(
            "raster of {total_pixels} pixels does not fit in memory on this target"
        ))
    })?;

    let mut masks = Vec::with_capacity(scan.band_count);
    for band_idx in 0..scan.band_count {
        let declared = declared_per_band.get(band_idx).copied().flatten();
        let mut bitmap = vec![false; pixels];
        let mut nodata_count = 0u64;

        scan_band(reader, scan, band_idx, |first_row, samples| {
            let base = usize::try_from(first_row * scan.width).map_err(|_| {
                crate::error::QcError::RasterError("row offset overflows usize".to_string())
            })?;
            for (offset, sample) in samples.chunks_exact(scan.bytes_per_sample).enumerate() {
                if matches_nodata(
                    sample,
                    scan.data_type,
                    scan.sample_format,
                    declared,
                    validator,
                ) {
                    // `scan_band` yields whole rows, so the stripe's samples are
                    // exactly `bitmap[base..base + n]`.
                    if let Some(cell) = bitmap.get_mut(base + offset) {
                        *cell = true;
                        nodata_count += 1;
                    }
                }
            }
            Ok(())
        })?;

        masks.push(NoDataBandMask {
            bitmap,
            total: total_pixels,
            nodata_count,
            suspected_unmarked_nodata: false,
        });
    }

    Ok(masks)
}

/// Tests whether `sample_bytes` matches the declared NoData sentinel for a
/// given (data_type, sample_format).
///
/// `sample_bytes` is in the **host's** byte order. The driver normalises decoded
/// samples once, on the way out of block decode, so a `MM` file and its `II`
/// twin deliver identical bytes here. This function must therefore not consult
/// the file's byte order at all: it used to (correctly, while the driver handed
/// back file-order bytes), and re-introducing that would byte-swap an `MM`
/// file's samples a second time and collapse its NoData count to zero
/// (cool-japan/oxigeo#14).
fn matches_nodata(
    sample_bytes: &[u8],
    dtype: RasterDataType,
    sample_format: SampleFormat,
    declared: Option<f64>,
    validator: &NoDataValidator,
) -> bool {
    let Some(decl) = declared else {
        return false;
    };

    use SampleFormat::{IeeeFloatingPoint, SignedInteger, UnsignedInteger};
    match (sample_format, dtype) {
        (UnsignedInteger, RasterDataType::UInt8) => {
            sample_bytes.first().is_some_and(|&v| v as f64 == decl)
        }
        (UnsignedInteger, RasterDataType::UInt16) => {
            native::read_u16(sample_bytes).is_some_and(|v| v as f64 == decl)
        }
        (UnsignedInteger, RasterDataType::UInt32) => {
            native::read_u32(sample_bytes).is_some_and(|v| v as f64 == decl)
        }
        (UnsignedInteger, RasterDataType::UInt64) => {
            native::read_u64(sample_bytes).is_some_and(|v| v as f64 == decl)
        }
        (SignedInteger, RasterDataType::Int8) => sample_bytes
            .first()
            .is_some_and(|&v| (v as i8) as f64 == decl),
        (SignedInteger, RasterDataType::Int16) => {
            native::read_i16(sample_bytes).is_some_and(|v| v as f64 == decl)
        }
        (SignedInteger, RasterDataType::Int32) => {
            native::read_i32(sample_bytes).is_some_and(|v| v as f64 == decl)
        }
        (SignedInteger, RasterDataType::Int64) => {
            native::read_i64(sample_bytes).is_some_and(|v| v as f64 == decl)
        }
        (IeeeFloatingPoint, RasterDataType::Float32) => {
            native::read_f32(sample_bytes).is_some_and(|v| {
                // Honour NaN-sentinel convention.
                if (decl as f32).is_nan() {
                    return v.is_nan();
                }
                (v - decl as f32).abs() <= validator.float_eps_f32
            })
        }
        (IeeeFloatingPoint, RasterDataType::Float64) => {
            native::read_f64(sample_bytes).is_some_and(|v| {
                if decl.is_nan() {
                    return v.is_nan();
                }
                (v - decl).abs() <= validator.float_eps_f64
            })
        }
        _ => false,
    }
}

/// Helper exposed for tests — synthesises a NoData mask directly.
#[cfg(test)]
pub(crate) fn mask_from_bools(bits: Vec<bool>) -> NoDataBandMask {
    let total = bits.len() as u64;
    let count = bits.iter().filter(|b| **b).count() as u64;
    NoDataBandMask {
        bitmap: bits,
        total,
        nodata_count: count,
        suspected_unmarked_nodata: false,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::float_cmp)]

    use super::*;

    fn validator() -> NoDataValidator {
        NoDataValidator::new()
    }

    #[test]
    fn test_nodata_consistent_across_bands_passes() {
        // Two bands, both declare NoData = -9999, both have matching pixels.
        let v = validator();
        let mask_a = mask_from_bools(vec![true, false, false, true]);
        let mask_b = mask_from_bools(vec![true, false, false, true]);
        let result = v.evaluate_masks(vec![mask_a, mask_b], vec![Some(-9999.0), Some(-9999.0)]);
        assert!(
            !result.issues.iter().any(|i| i.severity >= Severity::Major),
            "no Major issues expected; got {:#?}",
            result.issues
        );
        assert_eq!(result.common_footprint_count, 2);
    }

    #[test]
    fn test_nodata_metadata_without_pixels_warns() {
        let v = validator();
        let mask = mask_from_bools(vec![false, false, false, false]);
        let result = v.evaluate_masks(vec![mask], vec![Some(-9999.0)]);
        assert!(
            result.issues.iter().any(|i| i.severity == Severity::Warning
                && i.rule_id.as_deref() == Some("NODATA-METADATA-WITHOUT-PIXELS")),
            "expected Warning, got {:#?}",
            result.issues
        );
    }

    #[test]
    fn test_nodata_pixels_without_metadata_majors() {
        let v = validator();
        let mut mask = mask_from_bools(vec![true, true, false, false]);
        mask.suspected_unmarked_nodata = true;
        let result = v.evaluate_masks(vec![mask], vec![None]);
        assert!(
            result.issues.iter().any(|i| i.severity == Severity::Major
                && i.rule_id.as_deref() == Some("NODATA-PIXELS-WITHOUT-METADATA")),
            "expected Major, got {:#?}",
            result.issues
        );
    }

    #[test]
    fn test_nodata_values_differ_majors() {
        let v = validator();
        let mask_a = mask_from_bools(vec![true, false]);
        let mask_b = mask_from_bools(vec![false, true]);
        let result = v.evaluate_masks(vec![mask_a, mask_b], vec![Some(-9999.0), Some(0.0)]);
        assert!(
            result.issues.iter().any(|i| i.severity == Severity::Major
                && i.rule_id.as_deref() == Some("NODATA-VALUES-DIFFER")),
            "expected Major, got {:#?}",
            result.issues
        );
    }

    #[test]
    fn test_nodata_common_footprint_outlier_warns() {
        // Band 1 and 2 share a large NoData footprint; band 3 has nearly none.
        let v = validator();
        let mask_a = mask_from_bools(vec![true, true, true, true, true, false, false, false]);
        let mask_b = mask_from_bools(vec![true, true, true, true, true, false, false, false]);
        let mask_c = mask_from_bools(vec![false, false, false, false, false, false, false, false]);
        let result = v.evaluate_masks(
            vec![mask_a, mask_b, mask_c],
            vec![Some(-9999.0), Some(-9999.0), Some(-9999.0)],
        );
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.rule_id.as_deref() == Some("NODATA-COMMON-FOOTPRINT-OUTLIER")),
            "expected outlier Warning, got {:#?}",
            result.issues
        );
    }

    #[test]
    fn test_nodata_float_eps_tolerance() {
        // Pixel value differs from the declared NoData by 0.01: well within
        // a 0.1 epsilon, well outside the default 1e-6 epsilon.
        //
        // `to_ne_bytes`, not `to_le_bytes`: `matches_nodata` consumes samples in
        // the host's byte order, because that is what the driver hands out.
        let pixel: f32 = -9998.99;
        let bytes = pixel.to_ne_bytes();

        let lenient = NoDataValidator::new().with_float_eps_f32(0.1);
        assert!(
            matches_nodata(
                &bytes,
                RasterDataType::Float32,
                SampleFormat::IeeeFloatingPoint,
                Some(-9999.0),
                &lenient,
            ),
            "f32 pixel within lenient eps should match"
        );

        let strict = NoDataValidator::new(); // default 1e-6
        assert!(
            !matches_nodata(
                &bytes,
                RasterDataType::Float32,
                SampleFormat::IeeeFloatingPoint,
                Some(-9999.0),
                &strict,
            ),
            "outside strict eps should miss"
        );

        // NaN sentinel handling.
        let nan_pixel: f32 = f32::NAN;
        let nan_bytes = nan_pixel.to_ne_bytes();
        let nan_match = matches_nodata(
            &nan_bytes,
            RasterDataType::Float32,
            SampleFormat::IeeeFloatingPoint,
            Some(f64::NAN),
            &strict,
        );
        assert!(nan_match, "NaN pixel matches NaN sentinel");
    }

    /// A short sample slice must miss, not panic — `native::read_*` returns
    /// `None` rather than indexing past the end.
    #[test]
    fn test_nodata_truncated_sample_does_not_match() {
        let strict = NoDataValidator::new();
        for dtype in [
            RasterDataType::UInt16,
            RasterDataType::Int32,
            RasterDataType::Float64,
        ] {
            let fmt = match dtype {
                RasterDataType::UInt16 => SampleFormat::UnsignedInteger,
                RasterDataType::Int32 => SampleFormat::SignedInteger,
                _ => SampleFormat::IeeeFloatingPoint,
            };
            assert!(
                !matches_nodata(&[0u8], dtype, fmt, Some(0.0), &strict),
                "{dtype:?}: a one-byte slice cannot hold a sample"
            );
        }
    }
}

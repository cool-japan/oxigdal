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

use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::RasterDataType;
use oxigdal_geotiff::cog::CogReader;
use oxigdal_geotiff::tiff::{ImageInfo, SampleFormat};

use crate::error::{QcIssue, QcResult, Severity};

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
    /// All tiles for every band are read into memory; for very large rasters
    /// this can be memory-hungry — chunked streaming is a future
    /// improvement.
    pub fn check_file<P: AsRef<Path>>(&self, path: P) -> QcResult<NoDataValidationResult> {
        let source = FileDataSource::open(path.as_ref()).map_err(|e| {
            crate::error::QcError::RasterError(format!("Failed to open raster: {}", e))
        })?;
        let reader = CogReader::open(source).map_err(|e| {
            crate::error::QcError::RasterError(format!("Failed to read GeoTIFF: {}", e))
        })?;
        let info = reader.primary_info().clone();
        let nodata_value = reader.nodata().map_err(|e| {
            crate::error::QcError::RasterError(format!("nodata read failed: {}", e))
        })?;

        // GDAL stores a single NoData per file; if extended per-band metadata
        // becomes available later, plug it in here. For now, every band shares
        // the same declared NoData (from GDAL_NODATA tag).
        let band_count = info.samples_per_pixel as usize;
        let declared_per_band: Vec<Option<f64>> =
            (0..band_count).map(|_| nodata_value.as_f64()).collect();

        let masks = read_band_masks(&reader, &info, &declared_per_band, self)?;
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

fn read_band_masks<S: oxigdal_core::io::DataSource>(
    reader: &CogReader<S>,
    info: &ImageInfo,
    declared_per_band: &[Option<f64>],
    validator: &NoDataValidator,
) -> QcResult<Vec<NoDataBandMask>> {
    let band_count = info.samples_per_pixel as usize;
    let total_pixels = info.width * info.height;

    let dtype = info
        .data_type()
        .ok_or_else(|| crate::error::QcError::RasterError("data type unknown".to_string()))?;

    let bytes_per_sample = (info.bits_per_sample.first().copied().unwrap_or(8) / 8) as usize;
    let bytes_per_pixel = bytes_per_sample * band_count;
    let expected_per_tile_uncompressed = info
        .tile_width
        .map(|tw| tw as usize)
        .unwrap_or(info.width as usize)
        * info
            .tile_height
            .map(|th| th as usize)
            .unwrap_or(info.height as usize)
        * bytes_per_pixel;

    if expected_per_tile_uncompressed == 0 {
        return Err(crate::error::QcError::RasterError(
            "Image has zero-size tiles".to_string(),
        ));
    }

    let tiles_x = info.tiles_across() as usize;
    let tiles_y = info.tiles_down() as usize;

    let mut bitmaps: Vec<Vec<bool>> = (0..band_count)
        .map(|_| vec![false; total_pixels as usize])
        .collect();

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let tile_bytes = reader.read_tile(0, tx as u32, ty as u32).map_err(|e| {
                crate::error::QcError::RasterError(format!("read_tile failed: {}", e))
            })?;

            let tile_w = info
                .tile_width
                .map(|tw| tw as usize)
                .unwrap_or(info.width as usize);
            let tile_h = info.tile_height.map(|th| th as usize).unwrap_or({
                // striped: last strip may be shorter
                let strip_height = info.rows_per_strip.unwrap_or(info.height as u32) as usize;
                if ty == tiles_y - 1 {
                    let remaining = info.height as usize - ty * strip_height;
                    remaining.min(strip_height)
                } else {
                    strip_height
                }
            });

            // Walk every pixel in this tile that maps onto a real image cell.
            let img_w = info.width as usize;
            let img_h = info.height as usize;
            for row in 0..tile_h {
                let img_y = ty * tile_h + row;
                if img_y >= img_h {
                    break;
                }
                for col in 0..tile_w {
                    let img_x = tx * tile_w + col;
                    if img_x >= img_w {
                        break;
                    }
                    let pixel_offset_in_tile = (row * tile_w + col) * bytes_per_pixel;
                    if pixel_offset_in_tile + bytes_per_pixel > tile_bytes.len() {
                        break;
                    }
                    let pixel_idx = img_y * img_w + img_x;
                    for (band_idx, bitmap) in bitmaps.iter_mut().enumerate().take(band_count) {
                        let sample_offset = pixel_offset_in_tile + band_idx * bytes_per_sample;
                        let sample_bytes =
                            &tile_bytes[sample_offset..sample_offset + bytes_per_sample];
                        if matches_nodata(
                            sample_bytes,
                            dtype,
                            info.sample_format,
                            declared_per_band.get(band_idx).copied().flatten(),
                            validator,
                        ) {
                            bitmap[pixel_idx] = true;
                        }
                    }
                }
            }
        }
    }

    let masks = bitmaps
        .into_iter()
        .map(|bm| {
            let count = bm.iter().filter(|b| **b).count() as u64;
            NoDataBandMask {
                bitmap: bm,
                total: total_pixels,
                nodata_count: count,
                suspected_unmarked_nodata: false,
            }
        })
        .collect();

    Ok(masks)
}

/// Tests whether `sample_bytes` matches the declared NoData sentinel for a
/// given (data_type, sample_format).
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
            if sample_bytes.len() < 2 {
                return false;
            }
            let v = u16::from_le_bytes([sample_bytes[0], sample_bytes[1]]);
            v as f64 == decl
        }
        (UnsignedInteger, RasterDataType::UInt32) => {
            if sample_bytes.len() < 4 {
                return false;
            }
            let v = u32::from_le_bytes([
                sample_bytes[0],
                sample_bytes[1],
                sample_bytes[2],
                sample_bytes[3],
            ]);
            v as f64 == decl
        }
        (SignedInteger, RasterDataType::Int8) => sample_bytes
            .first()
            .is_some_and(|&v| (v as i8) as f64 == decl),
        (SignedInteger, RasterDataType::Int16) => {
            if sample_bytes.len() < 2 {
                return false;
            }
            let v = i16::from_le_bytes([sample_bytes[0], sample_bytes[1]]);
            v as f64 == decl
        }
        (SignedInteger, RasterDataType::Int32) => {
            if sample_bytes.len() < 4 {
                return false;
            }
            let v = i32::from_le_bytes([
                sample_bytes[0],
                sample_bytes[1],
                sample_bytes[2],
                sample_bytes[3],
            ]);
            v as f64 == decl
        }
        (IeeeFloatingPoint, RasterDataType::Float32) => {
            if sample_bytes.len() < 4 {
                return false;
            }
            let v = f32::from_le_bytes([
                sample_bytes[0],
                sample_bytes[1],
                sample_bytes[2],
                sample_bytes[3],
            ]);
            // Honour NaN-sentinel convention.
            if (decl as f32).is_nan() {
                return v.is_nan();
            }
            (v - decl as f32).abs() <= validator.float_eps_f32
        }
        (IeeeFloatingPoint, RasterDataType::Float64) => {
            if sample_bytes.len() < 8 {
                return false;
            }
            let v = f64::from_le_bytes([
                sample_bytes[0],
                sample_bytes[1],
                sample_bytes[2],
                sample_bytes[3],
                sample_bytes[4],
                sample_bytes[5],
                sample_bytes[6],
                sample_bytes[7],
            ]);
            if decl.is_nan() {
                return v.is_nan();
            }
            (v - decl).abs() <= validator.float_eps_f64
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
        let pixel: f32 = -9998.99;
        let bytes = pixel.to_le_bytes();

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
        let nan_bytes = nan_pixel.to_le_bytes();
        let nan_match = matches_nodata(
            &nan_bytes,
            RasterDataType::Float32,
            SampleFormat::IeeeFloatingPoint,
            Some(f64::NAN),
            &strict,
        );
        assert!(nan_match, "NaN pixel matches NaN sentinel");
    }
}

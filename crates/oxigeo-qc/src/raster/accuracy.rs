//! Raster data positional and thematic accuracy checks.
//!
//! This module provides quality control checks for raster data accuracy,
//! including georeferencing accuracy, GCP validation, and resolution validation.

use crate::error::{QcError, QcIssue, QcResult, Severity};
use oxigeo_core::buffer::{BufferStatistics, RasterBuffer};
use oxigeo_core::types::{BoundingBox, GeoTransform, SpatialReference};

/// Result of raster accuracy analysis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccuracyResult {
    /// Georeferencing accuracy assessment.
    pub georef_accuracy: GeoreferencingAccuracy,

    /// Ground control point validation results.
    pub gcp_validation: Option<GcpValidation>,

    /// Resolution validation result.
    pub resolution_check: ResolutionCheck,

    /// DEM accuracy assessment (if applicable).
    pub dem_accuracy: Option<DemAccuracy>,

    /// Orthorectification quality (if applicable).
    pub ortho_quality: Option<OrthoQuality>,

    /// Quality control issues found.
    pub issues: Vec<QcIssue>,
}

/// Georeferencing accuracy assessment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GeoreferencingAccuracy {
    /// Whether geotransform is valid.
    pub has_valid_geotransform: bool,

    /// Whether coordinate system is defined.
    pub has_coordinate_system: bool,

    /// Pixel size in X direction.
    pub pixel_size_x: f64,

    /// Pixel size in Y direction.
    pub pixel_size_y: f64,

    /// Whether pixel size is reasonable.
    pub reasonable_pixel_size: bool,

    /// Rotation/skew present.
    pub has_rotation: bool,

    /// Georeferencing quality assessment.
    pub quality: GeoreferenceQuality,
}

/// Georeferencing quality levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GeoreferenceQuality {
    /// Excellent georeferencing.
    Excellent,

    /// Good georeferencing.
    Good,

    /// Fair georeferencing.
    Fair,

    /// Poor georeferencing or missing.
    Poor,

    /// No georeferencing information.
    None,
}

/// Ground control point validation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GcpValidation {
    /// Number of GCPs.
    pub gcp_count: usize,

    /// Root mean square error in X direction.
    pub rmse_x: f64,

    /// Root mean square error in Y direction.
    pub rmse_y: f64,

    /// Overall RMSE.
    pub rmse_total: f64,

    /// Maximum residual error.
    pub max_error: f64,

    /// Whether GCP accuracy meets threshold.
    pub meets_threshold: bool,

    /// GCP distribution quality.
    pub distribution_quality: DistributionQuality,
}

/// GCP distribution quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DistributionQuality {
    /// Well-distributed GCPs.
    WellDistributed,

    /// Adequate distribution.
    Adequate,

    /// Poor distribution.
    Poor,

    /// Clustered GCPs.
    Clustered,
}

/// Resolution validation result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolutionCheck {
    /// Actual pixel resolution in X direction.
    pub actual_resolution_x: f64,

    /// Actual pixel resolution in Y direction.
    pub actual_resolution_y: f64,

    /// Expected pixel resolution (if known).
    pub expected_resolution: Option<f64>,

    /// Whether resolution is isotropic (square pixels).
    pub is_isotropic: bool,

    /// Resolution deviation percentage.
    pub resolution_deviation: Option<f64>,

    /// Whether resolution meets requirements.
    pub meets_requirements: bool,
}

/// DEM accuracy assessment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DemAccuracy {
    /// Elevation range (max - min).
    pub elevation_range: f64,

    /// Minimum elevation.
    pub min_elevation: f64,

    /// Maximum elevation.
    pub max_elevation: f64,

    /// Whether elevation values are reasonable.
    pub reasonable_elevations: bool,

    /// Estimated vertical accuracy (if known).
    pub vertical_accuracy: Option<f64>,

    /// Presence of artifacts (pits/peaks).
    pub has_artifacts: bool,
}

/// Orthorectification quality assessment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrthoQuality {
    /// Overall quality score (0.0 - 1.0).
    pub quality_score: f64,

    /// Estimated geometric accuracy.
    pub geometric_accuracy: f64,

    /// Presence of distortion artifacts.
    pub has_distortion: bool,

    /// Quality assessment.
    pub assessment: OrthoAssessment,
}

/// Orthorectification quality assessment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OrthoAssessment {
    /// Excellent orthorectification.
    Excellent,

    /// Good orthorectification.
    Good,

    /// Fair orthorectification.
    Fair,

    /// Poor orthorectification.
    Poor,
}

/// Configuration for accuracy checks.
#[derive(Debug, Clone)]
pub struct AccuracyConfig {
    /// Expected pixel resolution (None for no check).
    pub expected_resolution: Option<f64>,

    /// Maximum allowed resolution deviation (percentage).
    pub max_resolution_deviation: f64,

    /// GCP RMSE threshold.
    pub gcp_rmse_threshold: f64,

    /// Minimum number of GCPs required.
    pub min_gcp_count: usize,

    /// Expected elevation range for DEM (None for no check).
    pub expected_elevation_range: Option<(f64, f64)>,

    /// Whether to check for DEM artifacts.
    pub check_dem_artifacts: bool,

    /// Whether to assess orthorectification quality.
    pub assess_ortho_quality: bool,

    /// Minimum elevation difference (in the DEM's own vertical units) between
    /// a pixel and *every* one of its 8 valid neighbors for that pixel to be
    /// counted as a candidate pit/peak artifact. `None` derives a threshold
    /// from the buffer's own statistics at check time (a multiple of the
    /// standard deviation), so the check self-calibrates to the DEM's
    /// elevation units instead of assuming meters.
    pub dem_artifact_threshold: Option<f64>,

    /// Fraction of valid pixels that must be flagged as local extrema before
    /// `has_artifacts` is reported as `true`. Guards against a single noisy
    /// pixel or a nodata border triggering a false positive.
    pub dem_artifact_pixel_fraction: f64,
}

impl Default for AccuracyConfig {
    fn default() -> Self {
        Self {
            expected_resolution: None,
            max_resolution_deviation: 10.0,
            gcp_rmse_threshold: 1.0,
            min_gcp_count: 4,
            expected_elevation_range: None,
            check_dem_artifacts: true,
            assess_ortho_quality: false,
            dem_artifact_threshold: None,
            dem_artifact_pixel_fraction: 0.001,
        }
    }
}

/// Raster accuracy checker.
pub struct AccuracyChecker {
    config: AccuracyConfig,
}

impl AccuracyChecker {
    /// Creates a new accuracy checker with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: AccuracyConfig::default(),
        }
    }

    /// Creates a new accuracy checker with custom configuration.
    #[must_use]
    pub fn with_config(config: AccuracyConfig) -> Self {
        Self { config }
    }

    /// Checks accuracy of a raster with geotransform.
    ///
    /// `crs`, if supplied by the caller (e.g. from a `Dataset` wrapper that
    /// tracks the raster's coordinate reference system separately from
    /// [`RasterBuffer`], which has no CRS field of its own), is used to
    /// determine [`GeoreferencingAccuracy::has_coordinate_system`] honestly.
    /// Passing `None` means "the caller could not supply CRS information",
    /// which is reported as *not* having a coordinate system rather than
    /// silently assuming one exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the analysis fails.
    pub fn check_raster(
        &self,
        buffer: &RasterBuffer,
        geotransform: &GeoTransform,
        _bbox: Option<&BoundingBox>,
        crs: Option<&SpatialReference>,
    ) -> QcResult<AccuracyResult> {
        let mut issues = Vec::new();

        // Check georeferencing accuracy
        let georef_accuracy = self.check_georeferencing(buffer, geotransform, crs)?;
        if matches!(
            georef_accuracy.quality,
            GeoreferenceQuality::Poor | GeoreferenceQuality::None
        ) {
            issues.push(
                QcIssue::new(
                    Severity::Critical,
                    "accuracy",
                    "Poor or missing georeferencing",
                    format!("Georeferencing quality: {:?}", georef_accuracy.quality),
                )
                .with_suggestion("Verify geotransform and coordinate system definition"),
            );
        }

        // Check resolution
        let resolution_check = self.check_resolution(geotransform)?;
        if !resolution_check.meets_requirements {
            issues.push(
                QcIssue::new(
                    Severity::Minor,
                    "accuracy",
                    "Resolution does not meet requirements",
                    format!(
                        "Resolution deviation: {:?}%",
                        resolution_check.resolution_deviation
                    ),
                )
                .with_suggestion("Verify expected resolution and processing parameters"),
            );
        }

        if !resolution_check.is_isotropic {
            issues.push(
                QcIssue::new(
                    Severity::Warning,
                    "accuracy",
                    "Non-isotropic pixels detected",
                    format!(
                        "Pixel size X: {:.6}, Y: {:.6}",
                        resolution_check.actual_resolution_x, resolution_check.actual_resolution_y
                    ),
                )
                .with_suggestion("Consider resampling to square pixels if required"),
            );
        }

        // DEM accuracy check (if elevation data)
        let dem_accuracy = self.check_dem_accuracy(buffer)?;
        if let Some(ref dem) = dem_accuracy {
            if !dem.reasonable_elevations {
                issues.push(
                    QcIssue::new(
                        Severity::Major,
                        "accuracy",
                        "Unreasonable elevation values detected",
                        format!(
                            "Elevation range: {:.2} (min: {:.2}, max: {:.2})",
                            dem.elevation_range, dem.min_elevation, dem.max_elevation
                        ),
                    )
                    .with_suggestion("Verify elevation data source and units"),
                );
            }

            if dem.has_artifacts {
                issues.push(
                    QcIssue::new(
                        Severity::Minor,
                        "accuracy",
                        "DEM artifacts detected",
                        "Suspicious pits or peaks found in elevation data",
                    )
                    .with_suggestion("Apply artifact removal filter or manual editing"),
                );
            }
        }

        Ok(AccuracyResult {
            georef_accuracy,
            gcp_validation: None, // Would require GCP data
            resolution_check,
            dem_accuracy,
            ortho_quality: None, // Would require ortho-specific checks
            issues,
        })
    }

    /// Checks georeferencing accuracy.
    ///
    /// `crs` reflects whatever coordinate reference system the caller could
    /// supply (see [`Self::check_raster`]'s docs); [`RasterBuffer`] itself
    /// carries no CRS field, so this cannot be derived from `_buffer` alone.
    fn check_georeferencing(
        &self,
        _buffer: &RasterBuffer,
        geotransform: &GeoTransform,
        crs: Option<&SpatialReference>,
    ) -> QcResult<GeoreferencingAccuracy> {
        let pixel_size_x = geotransform.pixel_width.abs();
        let pixel_size_y = geotransform.pixel_height.abs();

        // Check if pixel size is reasonable (not zero, not too small, not too large)
        let reasonable_pixel_size = pixel_size_x > 1e-10
            && pixel_size_y > 1e-10
            && pixel_size_x < 1e10
            && pixel_size_y < 1e10;

        // Check for rotation/skew
        let has_rotation =
            geotransform.row_rotation.abs() > 1e-10 || geotransform.col_rotation.abs() > 1e-10;

        // A raster's pixel geometry can look perfectly sane while still
        // carrying no coordinate system at all (or an unparseable one),
        // which makes any coordinates it reports meaningless for absolute
        // positioning. `crs` is `None` whenever the caller could not (or did
        // not) supply CRS information -- this is reported honestly rather
        // than assumed to be present.
        let has_coordinate_system = crs.is_some();

        // Determine overall quality
        let quality = if !reasonable_pixel_size {
            GeoreferenceQuality::None
        } else if !has_coordinate_system {
            GeoreferenceQuality::Poor
        } else if has_rotation {
            GeoreferenceQuality::Fair
        } else if (pixel_size_x - pixel_size_y).abs() / pixel_size_x > 0.1 {
            GeoreferenceQuality::Good
        } else {
            GeoreferenceQuality::Excellent
        };

        Ok(GeoreferencingAccuracy {
            has_valid_geotransform: reasonable_pixel_size,
            has_coordinate_system,
            pixel_size_x,
            pixel_size_y,
            reasonable_pixel_size,
            has_rotation,
            quality,
        })
    }

    /// Checks resolution against expected values.
    fn check_resolution(&self, geotransform: &GeoTransform) -> QcResult<ResolutionCheck> {
        let actual_resolution_x = geotransform.pixel_width.abs();
        let actual_resolution_y = geotransform.pixel_height.abs();

        let is_isotropic =
            (actual_resolution_x - actual_resolution_y).abs() / actual_resolution_x < 0.01;

        let (resolution_deviation, meets_requirements) =
            if let Some(expected) = self.config.expected_resolution {
                let avg_resolution = (actual_resolution_x + actual_resolution_y) / 2.0;
                let deviation = ((avg_resolution - expected).abs() / expected) * 100.0;
                let meets = deviation <= self.config.max_resolution_deviation;
                (Some(deviation), meets)
            } else {
                (None, true)
            };

        Ok(ResolutionCheck {
            actual_resolution_x,
            actual_resolution_y,
            expected_resolution: self.config.expected_resolution,
            is_isotropic,
            resolution_deviation,
            meets_requirements,
        })
    }

    /// Validates ground control points.
    pub fn validate_gcps(&self, gcps: &[GroundControlPoint]) -> QcResult<GcpValidation> {
        if gcps.len() < self.config.min_gcp_count {
            return Err(QcError::ValidationRule(format!(
                "Insufficient GCPs: found {}, required {}",
                gcps.len(),
                self.config.min_gcp_count
            )));
        }

        // Calculate RMSE
        let mut sum_x_sq: f64 = 0.0;
        let mut sum_y_sq: f64 = 0.0;
        let mut max_error: f64 = 0.0;

        for gcp in gcps {
            let error_x = gcp.residual_x.abs();
            let error_y = gcp.residual_y.abs();
            sum_x_sq += error_x * error_x;
            sum_y_sq += error_y * error_y;
            max_error = max_error.max(error_x.max(error_y));
        }

        let n = gcps.len() as f64;
        let rmse_x = (sum_x_sq / n).sqrt();
        let rmse_y = (sum_y_sq / n).sqrt();
        let rmse_total = ((sum_x_sq + sum_y_sq) / n).sqrt();

        let meets_threshold = rmse_total <= self.config.gcp_rmse_threshold;

        // Assess GCP distribution
        let distribution_quality = self.assess_gcp_distribution(gcps);

        Ok(GcpValidation {
            gcp_count: gcps.len(),
            rmse_x,
            rmse_y,
            rmse_total,
            max_error,
            meets_threshold,
            distribution_quality,
        })
    }

    /// Assesses GCP spatial distribution.
    fn assess_gcp_distribution(&self, gcps: &[GroundControlPoint]) -> DistributionQuality {
        if gcps.len() < 4 {
            return DistributionQuality::Poor;
        }

        // Calculate bounding box and centroid
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;

        for gcp in gcps {
            min_x = min_x.min(gcp.pixel_x);
            max_x = max_x.max(gcp.pixel_x);
            min_y = min_y.min(gcp.pixel_y);
            max_y = max_y.max(gcp.pixel_y);
            sum_x += gcp.pixel_x;
            sum_y += gcp.pixel_y;
        }

        let centroid_x = sum_x / gcps.len() as f64;
        let centroid_y = sum_y / gcps.len() as f64;
        let range_x = max_x - min_x;
        let range_y = max_y - min_y;

        // Check if GCPs are clustered (all within 20% of extent)
        let threshold = 0.2;
        let clustered_x = range_x < threshold * (max_x + min_x) / 2.0;
        let clustered_y = range_y < threshold * (max_y + min_y) / 2.0;

        if clustered_x || clustered_y {
            return DistributionQuality::Clustered;
        }

        // Check distribution balance (centroid should be near center)
        let center_x = (min_x + max_x) / 2.0;
        let center_y = (min_y + max_y) / 2.0;
        let centroid_offset = ((centroid_x - center_x).powi(2) + (centroid_y - center_y).powi(2))
            .sqrt()
            / ((range_x.powi(2) + range_y.powi(2)).sqrt());

        if centroid_offset < 0.1 {
            DistributionQuality::WellDistributed
        } else if centroid_offset < 0.25 {
            DistributionQuality::Adequate
        } else {
            DistributionQuality::Poor
        }
    }

    /// Checks DEM accuracy.
    fn check_dem_accuracy(&self, buffer: &RasterBuffer) -> QcResult<Option<DemAccuracy>> {
        if !self.config.check_dem_artifacts {
            return Ok(None);
        }

        let stats = buffer.compute_statistics()?;

        if stats.valid_count == 0 {
            return Ok(None);
        }

        let elevation_range = stats.max - stats.min;

        // Check if elevations are reasonable
        let reasonable_elevations =
            if let Some((min_expected, max_expected)) = self.config.expected_elevation_range {
                stats.min >= min_expected && stats.max <= max_expected
            } else {
                // Default reasonableness check: -500m to 9000m (below sea to high mountains)
                stats.min >= -500.0 && stats.max <= 9000.0
            };

        let has_artifacts = self.detect_dem_artifacts(buffer, &stats)?;

        Ok(Some(DemAccuracy {
            elevation_range,
            min_elevation: stats.min,
            max_elevation: stats.max,
            reasonable_elevations,
            vertical_accuracy: None, // Would require reference data
            has_artifacts,
        }))
    }

    /// Detects DEM artifacts (pits/peaks) via an 8-neighborhood local-extrema scan.
    ///
    /// A pixel is a candidate pit/peak artifact when it differs from *every*
    /// one of its eight neighbors by more than `threshold`, and all eight
    /// differences share the same sign — the center is either lower than
    /// every neighbor (a pit) or higher than every neighbor (a peak).
    /// Pixels touching nodata are skipped entirely (all 8 neighbors must be
    /// valid data), which keeps nodata borders from generating false
    /// positives. The threshold defaults to `4 * std_dev` of the buffer's
    /// own elevation statistics (self-calibrating to the DEM's vertical
    /// units) unless `AccuracyConfig::dem_artifact_threshold` overrides it.
    /// `has_artifacts` is only reported `true` once the flagged-pixel count
    /// exceeds `dem_artifact_pixel_fraction` of the total valid pixel count,
    /// so a single noisy pixel in a large raster doesn't trip the check.
    fn detect_dem_artifacts(
        &self,
        buffer: &RasterBuffer,
        stats: &BufferStatistics,
    ) -> QcResult<bool> {
        let width = buffer.width();
        let height = buffer.height();

        // Need at least one full ring of neighbors around a center pixel.
        if width < 3 || height < 3 {
            return Ok(false);
        }

        let threshold = self
            .config
            .dem_artifact_threshold
            .unwrap_or(4.0 * stats.std_dev)
            .max(1e-9);

        let mut flagged = 0u64;

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let center = buffer.get_pixel(x, y)?;
                if buffer.is_nodata(center) || !center.is_finite() {
                    continue;
                }

                let mut diffs = [0.0f64; 8];
                let mut all_valid = true;
                let mut idx = 0usize;
                'neighbors: for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        // Safe: x, y range over [1, width-2] / [1, height-2],
                        // so x+dx and y+dy always land within [0, width-1] /
                        // [0, height-1].
                        let nx = (x as i64 + dx) as u64;
                        let ny = (y as i64 + dy) as u64;
                        let neighbor = buffer.get_pixel(nx, ny)?;
                        if buffer.is_nodata(neighbor) || !neighbor.is_finite() {
                            all_valid = false;
                            break 'neighbors;
                        }
                        if let Some(slot) = diffs.get_mut(idx) {
                            *slot = neighbor - center;
                        }
                        idx += 1;
                    }
                }

                if !all_valid {
                    continue;
                }

                let is_pit = diffs.iter().all(|&d| d > threshold);
                let is_peak = diffs.iter().all(|&d| d < -threshold);
                if is_pit || is_peak {
                    flagged += 1;
                }
            }
        }

        Ok(flagged as f64 > self.config.dem_artifact_pixel_fraction * stats.valid_count as f64)
    }
}

impl Default for AccuracyChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// Ground control point.
#[derive(Debug, Clone)]
pub struct GroundControlPoint {
    /// Pixel X coordinate.
    pub pixel_x: f64,

    /// Pixel Y coordinate.
    pub pixel_y: f64,

    /// Geographic X coordinate.
    pub geo_x: f64,

    /// Geographic Y coordinate.
    pub geo_y: f64,

    /// Residual error in X.
    pub residual_x: f64,

    /// Residual error in Y.
    pub residual_y: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigeo_core::types::RasterDataType;

    #[test]
    fn test_accuracy_checker_basic() {
        let buffer = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
        let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)
            .expect("Failed to create test bounding box");
        let geotransform = GeoTransform::from_bounds(&bbox, 1000, 1000)
            .expect("Failed to create test geotransform from bounds");
        let crs = SpatialReference::from_epsg(4326);

        let checker = AccuracyChecker::new();
        let result = checker.check_raster(&buffer, &geotransform, Some(&bbox), Some(&crs));

        assert!(result.is_ok());
        let result = result.expect("checked is_ok above");
        assert!(result.georef_accuracy.has_coordinate_system);
    }

    #[test]
    fn test_missing_crs_is_reported_honestly_not_hardcoded_true() {
        let buffer = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
        let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)
            .expect("Failed to create test bounding box");
        let geotransform = GeoTransform::from_bounds(&bbox, 1000, 1000)
            .expect("Failed to create test geotransform from bounds");

        let checker = AccuracyChecker::new();
        // No CRS supplied at all.
        let result = checker
            .check_raster(&buffer, &geotransform, Some(&bbox), None)
            .expect("check_raster should succeed even without CRS info");

        assert!(
            !result.georef_accuracy.has_coordinate_system,
            "has_coordinate_system must reflect the real absence of CRS info, not be hardcoded true"
        );
        assert_eq!(result.georef_accuracy.quality, GeoreferenceQuality::Poor);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.severity == Severity::Critical),
            "missing CRS combined with otherwise-valid geotransform must surface as a critical \
             georeferencing issue instead of a silently-green report"
        );
    }

    #[test]
    fn test_present_crs_allows_excellent_quality() {
        let buffer = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
        let bbox =
            BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("Failed to create test bounding box");
        let geotransform = GeoTransform::from_bounds(&bbox, 100, 100)
            .expect("Failed to create test geotransform from bounds");
        let crs = SpatialReference::from_epsg(3857);

        let checker = AccuracyChecker::new();
        let result = checker
            .check_raster(&buffer, &geotransform, Some(&bbox), Some(&crs))
            .expect("check_raster should succeed");

        assert!(result.georef_accuracy.has_coordinate_system);
        assert_eq!(
            result.georef_accuracy.quality,
            GeoreferenceQuality::Excellent
        );
    }

    #[test]
    fn test_resolution_check() {
        let bbox =
            BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("Failed to create test bounding box");
        let geotransform = GeoTransform::from_bounds(&bbox, 100, 100)
            .expect("Failed to create test geotransform from bounds");

        let checker = AccuracyChecker::new();
        let result = checker.check_resolution(&geotransform);

        assert!(result.is_ok());
        let result = result.expect("Resolution check should succeed");
        assert!(result.is_isotropic);
    }

    #[test]
    fn test_gcp_validation() {
        let gcps = vec![
            GroundControlPoint {
                pixel_x: 0.0,
                pixel_y: 0.0,
                geo_x: 0.0,
                geo_y: 0.0,
                residual_x: 0.1,
                residual_y: 0.1,
            },
            GroundControlPoint {
                pixel_x: 100.0,
                pixel_y: 0.0,
                geo_x: 1.0,
                geo_y: 0.0,
                residual_x: 0.2,
                residual_y: 0.1,
            },
            GroundControlPoint {
                pixel_x: 0.0,
                pixel_y: 100.0,
                geo_x: 0.0,
                geo_y: 1.0,
                residual_x: 0.1,
                residual_y: 0.2,
            },
            GroundControlPoint {
                pixel_x: 100.0,
                pixel_y: 100.0,
                geo_x: 1.0,
                geo_y: 1.0,
                residual_x: 0.15,
                residual_y: 0.15,
            },
        ];

        let checker = AccuracyChecker::new();
        let result = checker.validate_gcps(&gcps);

        assert!(result.is_ok());
        let result = result.expect("GCP validation should succeed");
        assert_eq!(result.gcp_count, 4);
        assert!(result.rmse_total < 1.0);
    }

    // ── DEM artifact (pit/peak) detection ──────────────────────────────────

    #[test]
    fn test_dem_artifacts_flat_surface_none_detected() {
        // Perfectly flat DEM: no local extrema anywhere, so has_artifacts
        // must stay false regardless of the (auto-derived) threshold.
        let buffer = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let checker = AccuracyChecker::new();
        let result = checker
            .check_dem_accuracy(&buffer)
            .expect("DEM accuracy check should succeed")
            .expect("flat DEM should yield a DemAccuracy result");

        assert!(
            !result.has_artifacts,
            "flat surface must not be flagged as having artifacts"
        );
    }

    #[test]
    fn test_dem_artifacts_single_deep_pit_detected() {
        // Flat 5x5 DEM at elevation 100.0 with a single deep pit (10.0) in
        // the center. In a small buffer, one flagged pixel is well above the
        // default 0.1% fraction threshold, so it must be reported.
        let mut buffer = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        buffer.fill_value(100.0);
        buffer
            .set_pixel(2, 2, 10.0)
            .expect("setting the pit pixel should succeed");

        let checker = AccuracyChecker::new();
        let result = checker
            .check_dem_accuracy(&buffer)
            .expect("DEM accuracy check should succeed")
            .expect("DEM with a pit should yield a DemAccuracy result");

        assert!(
            result.has_artifacts,
            "single-pixel pit exceeding the derived threshold should be detected"
        );
    }

    #[test]
    fn test_dem_artifacts_single_sharp_peak_detected() {
        // Same as the pit test, but the anomaly is a sharp peak instead.
        let mut buffer = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        buffer.fill_value(100.0);
        buffer
            .set_pixel(2, 2, 500.0)
            .expect("setting the peak pixel should succeed");

        let checker = AccuracyChecker::new();
        let result = checker
            .check_dem_accuracy(&buffer)
            .expect("DEM accuracy check should succeed")
            .expect("DEM with a peak should yield a DemAccuracy result");

        assert!(
            result.has_artifacts,
            "single-pixel peak exceeding the derived threshold should be detected"
        );
    }

    #[test]
    fn test_dem_artifacts_nodata_border_no_false_positive() {
        // Flat interior surrounded by a ring of nodata pixels. Border-
        // adjacent pixels can't be evaluated (a full 8-neighborhood of valid
        // data is required), and the flat interior has no local extrema, so
        // has_artifacts must stay false.
        let nodata = oxigeo_core::types::NoDataValue::Float(-9999.0);
        let mut buffer = RasterBuffer::nodata_filled(6, 6, RasterDataType::Float32, nodata);

        for y in 1..5u64 {
            for x in 1..5u64 {
                buffer
                    .set_pixel(x, y, 100.0)
                    .expect("setting interior pixel should succeed");
            }
        }

        let checker = AccuracyChecker::new();
        let result = checker
            .check_dem_accuracy(&buffer)
            .expect("DEM accuracy check should succeed")
            .expect("DEM with nodata border should yield a DemAccuracy result");

        assert!(
            !result.has_artifacts,
            "flat interior bordered by nodata must not be flagged"
        );
    }

    #[test]
    fn test_dem_artifacts_explicit_threshold_override() {
        // With an explicit threshold that no diff in this DEM can exceed,
        // even a fairly large bump must not be flagged.
        let mut buffer = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        buffer.fill_value(100.0);
        buffer
            .set_pixel(2, 2, 150.0)
            .expect("setting the bump pixel should succeed");

        let config = AccuracyConfig {
            dem_artifact_threshold: Some(1000.0),
            ..AccuracyConfig::default()
        };
        let checker = AccuracyChecker::with_config(config);

        let result = checker
            .check_dem_accuracy(&buffer)
            .expect("DEM accuracy check should succeed")
            .expect("DEM with a bump should yield a DemAccuracy result");

        assert!(
            !result.has_artifacts,
            "a diff smaller than an explicit override threshold must not be flagged"
        );
    }
}

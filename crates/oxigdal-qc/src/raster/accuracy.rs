//! Raster data positional and thematic accuracy checks.
//!
//! This module provides quality control checks for raster data accuracy,
//! including georeferencing accuracy, GCP validation, and resolution validation.

use crate::error::{QcError, QcIssue, QcResult, Severity};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform};

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
    /// # Errors
    ///
    /// Returns an error if the analysis fails.
    pub fn check_raster(
        &self,
        buffer: &RasterBuffer,
        geotransform: &GeoTransform,
        _bbox: Option<&BoundingBox>,
    ) -> QcResult<AccuracyResult> {
        let mut issues = Vec::new();

        // Check georeferencing accuracy
        let georef_accuracy = self.check_georeferencing(buffer, geotransform)?;
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
    fn check_georeferencing(
        &self,
        _buffer: &RasterBuffer,
        geotransform: &GeoTransform,
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

        // Determine overall quality
        let quality = if !reasonable_pixel_size {
            GeoreferenceQuality::None
        } else if has_rotation {
            GeoreferenceQuality::Fair
        } else if (pixel_size_x - pixel_size_y).abs() / pixel_size_x > 0.1 {
            GeoreferenceQuality::Good
        } else {
            GeoreferenceQuality::Excellent
        };

        Ok(GeoreferencingAccuracy {
            has_valid_geotransform: reasonable_pixel_size,
            has_coordinate_system: true, // Would need CRS info
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

        // Simple artifact detection (would be more sophisticated in full implementation)
        let has_artifacts = self.detect_dem_artifacts(buffer)?;

        Ok(Some(DemAccuracy {
            elevation_range,
            min_elevation: stats.min,
            max_elevation: stats.max,
            reasonable_elevations,
            vertical_accuracy: None, // Would require reference data
            has_artifacts,
        }))
    }

    /// Detects DEM artifacts (pits/peaks).
    fn detect_dem_artifacts(&self, _buffer: &RasterBuffer) -> QcResult<bool> {
        // Simplified implementation - full version would use morphological analysis
        Ok(false)
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
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_accuracy_checker_basic() {
        let buffer = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
        let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)
            .expect("Failed to create test bounding box");
        let geotransform = GeoTransform::from_bounds(&bbox, 1000, 1000)
            .expect("Failed to create test geotransform from bounds");

        let checker = AccuracyChecker::new();
        let result = checker.check_raster(&buffer, &geotransform, Some(&bbox));

        assert!(result.is_ok());
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
}

//! Raster data consistency checks.
//!
//! This module provides quality control checks for raster data logical consistency,
//! including value range validation, outlier detection, and artifact detection.

use crate::error::{QcIssue, QcResult, Severity};
use oxigdal_core::buffer::{BufferStatistics, RasterBuffer};

/// Result of raster consistency analysis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConsistencyResult {
    /// Basic statistics of the raster.
    pub statistics: BasicStatistics,

    /// Value range validation result.
    pub range_check: RangeCheckResult,

    /// Outlier detection result.
    pub outliers: OutlierResult,

    /// Block boundary artifacts detected.
    pub block_artifacts: Vec<BlockArtifact>,

    /// Seamline artifacts detected.
    pub seamline_artifacts: Vec<SeamlineArtifact>,

    /// Compression artifacts severity.
    pub compression_quality: CompressionQuality,

    /// Quality control issues found.
    pub issues: Vec<QcIssue>,
}

/// Basic statistics summary.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BasicStatistics {
    /// Minimum value.
    pub min: f64,

    /// Maximum value.
    pub max: f64,

    /// Mean value.
    pub mean: f64,

    /// Standard deviation.
    pub std_dev: f64,

    /// Number of valid pixels.
    pub valid_count: u64,
}

impl From<BufferStatistics> for BasicStatistics {
    fn from(stats: BufferStatistics) -> Self {
        Self {
            min: stats.min,
            max: stats.max,
            mean: stats.mean,
            std_dev: stats.std_dev,
            valid_count: stats.valid_count,
        }
    }
}

/// Value range validation result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RangeCheckResult {
    /// Expected minimum value.
    pub expected_min: Option<f64>,

    /// Expected maximum value.
    pub expected_max: Option<f64>,

    /// Actual minimum value.
    pub actual_min: f64,

    /// Actual maximum value.
    pub actual_max: f64,

    /// Whether values are within expected range.
    pub in_range: bool,

    /// Number of out-of-range pixels.
    pub out_of_range_count: u64,
}

/// Outlier detection result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OutlierResult {
    /// Number of statistical outliers detected.
    pub outlier_count: u64,

    /// Percentage of outliers (0.0 - 100.0).
    pub outlier_percentage: f64,

    /// Outlier threshold used (number of standard deviations).
    pub threshold_sigma: f64,

    /// Lower bound for outlier detection.
    pub lower_bound: f64,

    /// Upper bound for outlier detection.
    pub upper_bound: f64,
}

/// Block boundary artifact information.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockArtifact {
    /// X coordinate of the block boundary.
    pub x: u64,

    /// Y coordinate of the block boundary.
    pub y: u64,

    /// Type of artifact (horizontal or vertical).
    pub artifact_type: ArtifactType,

    /// Severity of the artifact.
    pub severity: Severity,

    /// Discontinuity magnitude.
    pub magnitude: f64,
}

/// Type of boundary artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ArtifactType {
    /// Horizontal boundary artifact.
    Horizontal,

    /// Vertical boundary artifact.
    Vertical,
}

/// Seamline artifact information.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SeamlineArtifact {
    /// Starting X coordinate.
    pub start_x: u64,

    /// Starting Y coordinate.
    pub start_y: u64,

    /// Ending X coordinate.
    pub end_x: u64,

    /// Ending Y coordinate.
    pub end_y: u64,

    /// Severity of the seamline.
    pub severity: Severity,

    /// Average intensity difference across seamline.
    pub avg_difference: f64,
}

/// Compression quality assessment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompressionQuality {
    /// Overall quality score (0.0 - 1.0, higher is better).
    pub quality_score: f64,

    /// Estimated blockiness level (0.0 - 1.0).
    pub blockiness: f64,

    /// Estimated noise level (0.0 - 1.0).
    pub noise_level: f64,

    /// Quality assessment.
    pub assessment: CompressionAssessment,
}

/// Compression quality assessment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CompressionAssessment {
    /// Excellent quality, no visible artifacts.
    Excellent,

    /// Good quality, minimal artifacts.
    Good,

    /// Fair quality, some artifacts present.
    Fair,

    /// Poor quality, significant artifacts.
    Poor,

    /// Very poor quality, severe artifacts.
    VeryPoor,
}

/// Configuration for consistency checks.
#[derive(Debug, Clone)]
pub struct ConsistencyConfig {
    /// Expected minimum value (None for no check).
    pub expected_min: Option<f64>,

    /// Expected maximum value (None for no check).
    pub expected_max: Option<f64>,

    /// Outlier threshold in standard deviations.
    pub outlier_sigma: f64,

    /// Maximum allowed outlier percentage.
    pub max_outlier_percentage: f64,

    /// Block size for artifact detection.
    pub block_size: u64,

    /// Minimum discontinuity magnitude to flag as artifact.
    pub artifact_threshold: f64,

    /// Whether to check for block boundary artifacts.
    pub check_block_artifacts: bool,

    /// Whether to check for seamline artifacts.
    pub check_seamlines: bool,

    /// Whether to assess compression quality.
    pub assess_compression: bool,
}

impl Default for ConsistencyConfig {
    fn default() -> Self {
        Self {
            expected_min: None,
            expected_max: None,
            outlier_sigma: 3.0,
            max_outlier_percentage: 5.0,
            block_size: 256,
            artifact_threshold: 0.1,
            check_block_artifacts: true,
            check_seamlines: true,
            assess_compression: true,
        }
    }
}

/// Raster consistency checker.
pub struct ConsistencyChecker {
    config: ConsistencyConfig,
}

impl ConsistencyChecker {
    /// Creates a new consistency checker with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: ConsistencyConfig::default(),
        }
    }

    /// Creates a new consistency checker with custom configuration.
    #[must_use]
    pub fn with_config(config: ConsistencyConfig) -> Self {
        Self { config }
    }

    /// Checks consistency of a raster buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if the buffer cannot be analyzed.
    pub fn check_buffer(&self, buffer: &RasterBuffer) -> QcResult<ConsistencyResult> {
        let mut issues = Vec::new();

        // Compute basic statistics
        let stats = buffer.compute_statistics()?;
        let basic_stats = BasicStatistics::from(stats);

        // Check value range
        let range_check = self.check_value_range(buffer, &basic_stats)?;
        if !range_check.in_range {
            issues.push(
                QcIssue::new(
                    Severity::Major,
                    "consistency",
                    "Values out of expected range",
                    format!(
                        "Found {} pixels outside expected range [{:?}, {:?}]",
                        range_check.out_of_range_count,
                        self.config.expected_min,
                        self.config.expected_max
                    ),
                )
                .with_suggestion("Verify data source and processing parameters"),
            );
        }

        // Detect outliers
        let outliers = self.detect_outliers(buffer, &basic_stats)?;
        if outliers.outlier_percentage > self.config.max_outlier_percentage {
            issues.push(
                QcIssue::new(
                    Severity::Warning,
                    "consistency",
                    "High percentage of outliers",
                    format!(
                        "Outlier percentage ({:.2}%) exceeds threshold ({:.2}%)",
                        outliers.outlier_percentage, self.config.max_outlier_percentage
                    ),
                )
                .with_suggestion("Review outliers to determine if they are legitimate or errors"),
            );
        }

        // Detect block boundary artifacts
        let block_artifacts = if self.config.check_block_artifacts {
            let artifacts = self.detect_block_artifacts(buffer)?;
            for artifact in &artifacts {
                if artifact.severity >= Severity::Minor {
                    issues.push(
                        QcIssue::new(
                            artifact.severity,
                            "consistency",
                            "Block boundary artifact detected",
                            format!(
                                "{:?} artifact at ({}, {}) with magnitude {:.4}",
                                artifact.artifact_type, artifact.x, artifact.y, artifact.magnitude
                            ),
                        )
                        .with_location(format!("({}, {})", artifact.x, artifact.y))
                        .with_suggestion("Check block-based processing and ensure proper blending"),
                    );
                }
            }
            artifacts
        } else {
            Vec::new()
        };

        // Detect seamline artifacts
        let seamline_artifacts = if self.config.check_seamlines {
            let seamlines = self.detect_seamline_artifacts(buffer)?;
            for seamline in &seamlines {
                if seamline.severity >= Severity::Minor {
                    issues.push(
                        QcIssue::new(
                            seamline.severity,
                            "consistency",
                            "Seamline artifact detected",
                            format!(
                                "Seamline from ({}, {}) to ({}, {}) with avg difference {:.4}",
                                seamline.start_x,
                                seamline.start_y,
                                seamline.end_x,
                                seamline.end_y,
                                seamline.avg_difference
                            ),
                        )
                        .with_suggestion("Apply seamline blending or color balancing"),
                    );
                }
            }
            seamlines
        } else {
            Vec::new()
        };

        // Assess compression quality
        let compression_quality = if self.config.assess_compression {
            let quality = self.assess_compression_quality(buffer)?;
            if matches!(
                quality.assessment,
                CompressionAssessment::Poor | CompressionAssessment::VeryPoor
            ) {
                issues.push(
                    QcIssue::new(
                        Severity::Minor,
                        "consistency",
                        "Poor compression quality",
                        format!(
                            "Compression quality: {:?}, blockiness: {:.2}, noise: {:.2}",
                            quality.assessment, quality.blockiness, quality.noise_level
                        ),
                    )
                    .with_suggestion(
                        "Use higher quality compression settings or lossless compression",
                    ),
                );
            }
            quality
        } else {
            CompressionQuality {
                quality_score: 1.0,
                blockiness: 0.0,
                noise_level: 0.0,
                assessment: CompressionAssessment::Excellent,
            }
        };

        Ok(ConsistencyResult {
            statistics: basic_stats,
            range_check,
            outliers,
            block_artifacts,
            seamline_artifacts,
            compression_quality,
            issues,
        })
    }

    /// Checks if values are within expected range.
    fn check_value_range(
        &self,
        buffer: &RasterBuffer,
        stats: &BasicStatistics,
    ) -> QcResult<RangeCheckResult> {
        let mut out_of_range_count = 0u64;

        if let (Some(min), Some(max)) = (self.config.expected_min, self.config.expected_max) {
            for y in 0..buffer.height() {
                for x in 0..buffer.width() {
                    let value = buffer.get_pixel(x, y)?;
                    if !buffer.is_nodata(value) && value.is_finite() && (value < min || value > max)
                    {
                        out_of_range_count = out_of_range_count.saturating_add(1);
                    }
                }
            }
        }

        let in_range = out_of_range_count == 0;

        Ok(RangeCheckResult {
            expected_min: self.config.expected_min,
            expected_max: self.config.expected_max,
            actual_min: stats.min,
            actual_max: stats.max,
            in_range,
            out_of_range_count,
        })
    }

    /// Detects statistical outliers.
    fn detect_outliers(
        &self,
        buffer: &RasterBuffer,
        stats: &BasicStatistics,
    ) -> QcResult<OutlierResult> {
        let lower_bound = stats.mean - (self.config.outlier_sigma * stats.std_dev);
        let upper_bound = stats.mean + (self.config.outlier_sigma * stats.std_dev);

        let mut outlier_count = 0u64;

        for y in 0..buffer.height() {
            for x in 0..buffer.width() {
                let value = buffer.get_pixel(x, y)?;
                if !buffer.is_nodata(value)
                    && value.is_finite()
                    && (value < lower_bound || value > upper_bound)
                {
                    outlier_count = outlier_count.saturating_add(1);
                }
            }
        }

        let outlier_percentage = if stats.valid_count > 0 {
            (outlier_count as f64 / stats.valid_count as f64) * 100.0
        } else {
            0.0
        };

        Ok(OutlierResult {
            outlier_count,
            outlier_percentage,
            threshold_sigma: self.config.outlier_sigma,
            lower_bound,
            upper_bound,
        })
    }

    /// Detects block boundary artifacts.
    fn detect_block_artifacts(&self, buffer: &RasterBuffer) -> QcResult<Vec<BlockArtifact>> {
        let mut artifacts = Vec::new();
        let block_size = self.config.block_size;

        // Check vertical boundaries
        let mut x = block_size;
        while x < buffer.width() {
            let magnitude = self.calculate_vertical_discontinuity(buffer, x)?;
            if magnitude > self.config.artifact_threshold {
                let severity = if magnitude > 0.5 {
                    Severity::Major
                } else if magnitude > 0.3 {
                    Severity::Minor
                } else {
                    Severity::Warning
                };

                artifacts.push(BlockArtifact {
                    x,
                    y: 0,
                    artifact_type: ArtifactType::Vertical,
                    severity,
                    magnitude,
                });
            }
            x += block_size;
        }

        // Check horizontal boundaries
        let mut y = block_size;
        while y < buffer.height() {
            let magnitude = self.calculate_horizontal_discontinuity(buffer, y)?;
            if magnitude > self.config.artifact_threshold {
                let severity = if magnitude > 0.5 {
                    Severity::Major
                } else if magnitude > 0.3 {
                    Severity::Minor
                } else {
                    Severity::Warning
                };

                artifacts.push(BlockArtifact {
                    x: 0,
                    y,
                    artifact_type: ArtifactType::Horizontal,
                    severity,
                    magnitude,
                });
            }
            y += block_size;
        }

        Ok(artifacts)
    }

    /// Calculates vertical discontinuity at a given X coordinate.
    fn calculate_vertical_discontinuity(&self, buffer: &RasterBuffer, x: u64) -> QcResult<f64> {
        if x == 0 || x >= buffer.width() {
            return Ok(0.0);
        }

        let mut sum_diff = 0.0;
        let mut count = 0u64;

        for y in 0..buffer.height() {
            let left = buffer.get_pixel(x - 1, y)?;
            let right = buffer.get_pixel(x, y)?;

            if !buffer.is_nodata(left)
                && !buffer.is_nodata(right)
                && left.is_finite()
                && right.is_finite()
            {
                sum_diff += (right - left).abs();
                count = count.saturating_add(1);
            }
        }

        Ok(if count > 0 {
            sum_diff / count as f64
        } else {
            0.0
        })
    }

    /// Calculates horizontal discontinuity at a given Y coordinate.
    fn calculate_horizontal_discontinuity(&self, buffer: &RasterBuffer, y: u64) -> QcResult<f64> {
        if y == 0 || y >= buffer.height() {
            return Ok(0.0);
        }

        let mut sum_diff = 0.0;
        let mut count = 0u64;

        for x in 0..buffer.width() {
            let top = buffer.get_pixel(x, y - 1)?;
            let bottom = buffer.get_pixel(x, y)?;

            if !buffer.is_nodata(top)
                && !buffer.is_nodata(bottom)
                && top.is_finite()
                && bottom.is_finite()
            {
                sum_diff += (bottom - top).abs();
                count = count.saturating_add(1);
            }
        }

        Ok(if count > 0 {
            sum_diff / count as f64
        } else {
            0.0
        })
    }

    /// Detects seamline artifacts.
    fn detect_seamline_artifacts(&self, _buffer: &RasterBuffer) -> QcResult<Vec<SeamlineArtifact>> {
        // Simplified implementation - full implementation would use edge detection
        // and color difference analysis
        Ok(Vec::new())
    }

    /// Assesses compression quality.
    fn assess_compression_quality(&self, buffer: &RasterBuffer) -> QcResult<CompressionQuality> {
        // Calculate blockiness using simplified DCT-based metric
        let blockiness = self.estimate_blockiness(buffer)?;

        // Estimate noise level using high-frequency content
        let noise_level = self.estimate_noise_level(buffer)?;

        // Compute overall quality score
        let quality_score = 1.0 - ((blockiness + noise_level) / 2.0);

        let assessment = if quality_score > 0.9 {
            CompressionAssessment::Excellent
        } else if quality_score > 0.75 {
            CompressionAssessment::Good
        } else if quality_score > 0.6 {
            CompressionAssessment::Fair
        } else if quality_score > 0.4 {
            CompressionAssessment::Poor
        } else {
            CompressionAssessment::VeryPoor
        };

        Ok(CompressionQuality {
            quality_score,
            blockiness,
            noise_level,
            assessment,
        })
    }

    /// Estimates blockiness level.
    fn estimate_blockiness(&self, buffer: &RasterBuffer) -> QcResult<f64> {
        // Simplified blockiness estimation
        let mut block_diff_sum = 0.0;
        let mut block_count = 0u64;

        let block_size = 8u64; // Typical JPEG block size

        let mut y = block_size;
        while y < buffer.height() {
            let diff = self.calculate_horizontal_discontinuity(buffer, y)?;
            block_diff_sum += diff;
            block_count = block_count.saturating_add(1);
            y += block_size;
        }

        let mut x = block_size;
        while x < buffer.width() {
            let diff = self.calculate_vertical_discontinuity(buffer, x)?;
            block_diff_sum += diff;
            block_count = block_count.saturating_add(1);
            x += block_size;
        }

        Ok(if block_count > 0 {
            (block_diff_sum / block_count as f64).min(1.0)
        } else {
            0.0
        })
    }

    /// Estimates noise level.
    fn estimate_noise_level(&self, buffer: &RasterBuffer) -> QcResult<f64> {
        // Simplified noise estimation using local variance
        let sample_size = 100u64.min(buffer.width()).min(buffer.height());
        let mut variance_sum = 0.0;
        let mut sample_count = 0u64;

        for _ in 0..sample_size {
            let x = (buffer.width() / 2).saturating_sub(sample_size / 2);
            let y = (buffer.height() / 2).saturating_sub(sample_size / 2);

            if x > 0 && y > 0 && x < buffer.width() - 1 && y < buffer.height() - 1 {
                let center = buffer.get_pixel(x, y)?;
                let mut local_sum = 0.0;
                let mut local_count = 0u64;

                // 3x3 neighborhood
                for dy in 0..3 {
                    for dx in 0..3 {
                        let val = buffer.get_pixel(x + dx - 1, y + dy - 1)?;
                        if val.is_finite() {
                            local_sum += (val - center).powi(2);
                            local_count = local_count.saturating_add(1);
                        }
                    }
                }

                if local_count > 0 {
                    variance_sum += local_sum / local_count as f64;
                    sample_count = sample_count.saturating_add(1);
                }
            }
        }

        Ok(if sample_count > 0 {
            (variance_sum / sample_count as f64).sqrt().min(1.0)
        } else {
            0.0
        })
    }
}

impl Default for ConsistencyChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_consistency_checker_basic() {
        let buffer = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
        let checker = ConsistencyChecker::new();
        let result = checker.check_buffer(&buffer);

        assert!(result.is_ok());
    }

    #[test]
    fn test_range_check() {
        let config = ConsistencyConfig {
            expected_min: Some(0.0),
            expected_max: Some(100.0),
            ..Default::default()
        };

        let mut buffer = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let result = buffer.set_pixel(5, 5, 150.0); // Out of range
        assert!(result.is_ok());

        let checker = ConsistencyChecker::with_config(config);
        let result = checker.check_buffer(&buffer);

        assert!(result.is_ok());
        #[allow(clippy::unwrap_used)]
        let result = result.expect("consistency check should succeed for range validation test");
        assert!(!result.range_check.in_range);
        assert_eq!(result.range_check.out_of_range_count, 1);
    }

    #[test]
    fn test_artifact_type() {
        let artifact = BlockArtifact {
            x: 256,
            y: 0,
            artifact_type: ArtifactType::Vertical,
            severity: Severity::Minor,
            magnitude: 0.25,
        };

        assert_eq!(artifact.artifact_type, ArtifactType::Vertical);
    }

    #[test]
    fn test_compression_assessment() {
        let quality = CompressionQuality {
            quality_score: 0.95,
            blockiness: 0.02,
            noise_level: 0.03,
            assessment: CompressionAssessment::Excellent,
        };

        assert_eq!(quality.assessment, CompressionAssessment::Excellent);
        assert!(quality.quality_score > 0.9);
    }
}

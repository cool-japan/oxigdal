//! Raster data consistency checks.
//!
//! This module provides quality control checks for raster data logical consistency,
//! including value range validation, outlier detection, and artifact detection.

use crate::error::{QcIssue, QcResult, Severity};
use oxigeo_core::buffer::{BufferStatistics, RasterBuffer};

/// Median of a slice of `f64` values (not modified in place; the slice is
/// copied and sorted internally). Returns `0.0` for an empty slice.
fn median_of(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

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

    /// Detects seamline artifacts: full-height/full-width lines of
    /// coordinated intensity discontinuity, the signature of two mosaicked
    /// source tiles meeting at an arbitrary boundary.
    ///
    /// Unlike [`Self::detect_block_artifacts`] (which only samples the
    /// periodic `block_size` grid characteristic of block-based compression
    /// artifacts), a mosaic seam can occur at *any* column or row, so every
    /// interior column and row is scanned using the same discontinuity
    /// metric ([`Self::calculate_vertical_discontinuity`] /
    /// [`Self::calculate_horizontal_discontinuity`]). A column/row is
    /// flagged as a seamline candidate when its discontinuity is a robust
    /// statistical outlier (via median + MAD, which tolerates the normal
    /// texture noise present in most rasters) relative to every other
    /// column/row in the same scan, and additionally exceeds the configured
    /// absolute `artifact_threshold` so a near-uniform image cannot trigger
    /// on float noise alone. Adjacent flagged positions are merged into a
    /// single seamline segment.
    fn detect_seamline_artifacts(&self, buffer: &RasterBuffer) -> QcResult<Vec<SeamlineArtifact>> {
        let width = buffer.width();
        let height = buffer.height();
        let mut artifacts = Vec::new();

        if width > 2 {
            let magnitudes: Vec<f64> = (1..width)
                .map(|x| self.calculate_vertical_discontinuity(buffer, x))
                .collect::<QcResult<Vec<_>>>()?;
            artifacts.extend(Self::find_seamlines_from_magnitudes(
                &magnitudes,
                1,
                self.config.artifact_threshold,
                |start_pos, end_pos| SeamlineArtifact {
                    start_x: start_pos,
                    start_y: 0,
                    end_x: end_pos,
                    end_y: height.saturating_sub(1),
                    severity: Severity::Info, // overwritten below
                    avg_difference: 0.0,      // overwritten below
                },
            ));
        }

        if height > 2 {
            let magnitudes: Vec<f64> = (1..height)
                .map(|y| self.calculate_horizontal_discontinuity(buffer, y))
                .collect::<QcResult<Vec<_>>>()?;
            artifacts.extend(Self::find_seamlines_from_magnitudes(
                &magnitudes,
                1,
                self.config.artifact_threshold,
                |start_pos, end_pos| SeamlineArtifact {
                    start_x: 0,
                    start_y: start_pos,
                    end_x: width.saturating_sub(1),
                    end_y: end_pos,
                    severity: Severity::Info, // overwritten below
                    avg_difference: 0.0,      // overwritten below
                },
            ));
        }

        Ok(artifacts)
    }

    /// Robust outlier scan over a 1-D array of discontinuity magnitudes
    /// (one entry per scanned column or row), merging adjacent flagged
    /// positions into single seamline segments.
    ///
    /// `make_artifact(start_pos, end_pos)` builds a [`SeamlineArtifact`] with
    /// the caller's choice of axis layout (vertical vs. horizontal); its
    /// `severity`/`avg_difference` placeholders are overwritten with the
    /// real computed values before being returned.
    fn find_seamlines_from_magnitudes(
        magnitudes: &[f64],
        index_offset: u64,
        artifact_threshold: f64,
        make_artifact: impl Fn(u64, u64) -> SeamlineArtifact,
    ) -> Vec<SeamlineArtifact> {
        if magnitudes.is_empty() {
            return Vec::new();
        }

        let median = median_of(magnitudes);
        let abs_devs: Vec<f64> = magnitudes.iter().map(|m| (m - median).abs()).collect();
        // Scale MAD by the standard consistency constant so it estimates a
        // normal-distribution standard deviation (same convention used by
        // `MadDetector` in oxigeo-observability's anomaly module).
        let robust_std = (median_of(&abs_devs) * 1.4826).max(1e-9);

        const SEAMLINE_ROBUST_Z_THRESHOLD: f64 = 4.0;

        let mut flagged: Vec<usize> = magnitudes
            .iter()
            .enumerate()
            .filter(|&(_, &m)| {
                let z = (m - median) / robust_std;
                z > SEAMLINE_ROBUST_Z_THRESHOLD && m > artifact_threshold
            })
            .map(|(i, _)| i)
            .collect();
        flagged.sort_unstable();

        let mut artifacts = Vec::new();
        let mut i = 0;
        while i < flagged.len() {
            let start_idx = flagged[i];
            let mut end_idx = start_idx;
            let mut sum = magnitudes[start_idx];
            let mut count = 1u64;

            while i + 1 < flagged.len() && flagged[i + 1] == end_idx + 1 {
                i += 1;
                end_idx = flagged[i];
                sum += magnitudes[end_idx];
                count += 1;
            }

            let avg_difference = sum / count as f64;
            let severity = if avg_difference > 0.5 {
                Severity::Major
            } else if avg_difference > 0.3 {
                Severity::Minor
            } else {
                Severity::Warning
            };

            let start_pos = index_offset + start_idx as u64;
            let end_pos = index_offset + end_idx as u64;

            let mut artifact = make_artifact(start_pos, end_pos);
            artifact.severity = severity;
            artifact.avg_difference = avg_difference;
            artifacts.push(artifact);

            i += 1;
        }

        artifacts
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
    /// Estimates noise level using local 3x3-neighborhood variance,
    /// stratified-sampled across the raster.
    ///
    /// Samples are laid out on a roughly square grid spanning the full
    /// interior of the raster (excluding the 1px border needed for the 3x3
    /// neighborhood), instead of recomputing the identical fixed off-center
    /// patch on every iteration (the previous bug: `x`/`y` were derived only
    /// from `buffer.width()`/`buffer.height()`/`sample_size`, never from the
    /// loop index, so every one of up to 100 iterations sampled the exact
    /// same 3x3 neighborhood).
    fn estimate_noise_level(&self, buffer: &RasterBuffer) -> QcResult<f64> {
        let width = buffer.width();
        let height = buffer.height();

        // Need at least a 3x3 interior (a 1px border on every side) to
        // sample a full neighborhood from.
        if width < 3 || height < 3 {
            return Ok(0.0);
        }

        let interior_width = width - 2;
        let interior_height = height - 2;
        let sample_size = 100u64
            .min(interior_width.saturating_mul(interior_height))
            .max(1);

        // Roughly square sampling grid covering the whole interior.
        let cols = (sample_size as f64).sqrt().ceil().max(1.0) as u64;
        let rows = sample_size.div_ceil(cols).max(1);

        let mut variance_sum = 0.0;
        let mut sample_count = 0u64;

        'sampling: for row in 0..rows {
            for col in 0..cols {
                if sample_count >= sample_size {
                    break 'sampling;
                }

                // Distinct cell-center coordinates per (row, col), spread
                // across the full interior instead of a single fixed point.
                let x = 1 + (col * interior_width / cols).min(interior_width.saturating_sub(1));
                let y = 1 + (row * interior_height / rows).min(interior_height.saturating_sub(1));

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
    use oxigeo_core::types::RasterDataType;

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
        #[allow(clippy::unwrap_used, clippy::expect_used)]
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

    #[test]
    fn test_detect_seamline_artifacts_finds_a_real_vertical_seam() {
        // Build a 60x60 image where columns [0, 30) are a constant low value
        // and columns [30, 60) are a constant, very different high value --
        // a textbook mosaic seam at x=30. Small texture noise elsewhere in
        // each half should not itself be mistaken for the seam.
        let width = 60u64;
        let height = 60u64;
        let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

        for y in 0..height {
            for x in 0..width {
                let base = if x < 30 { 10.0 } else { 200.0 };
                // Deterministic tiny texture noise so all columns aren't
                // *perfectly* uniform (which would make every discontinuity
                // exactly 0.0 except at the seam -- still a valid case, but
                // this is closer to a realistic raster).
                let noise = ((x * 7 + y * 13) % 3) as f64 * 0.01;
                buffer
                    .set_pixel(x, y, base + noise)
                    .expect("set_pixel should succeed for in-bounds coordinates");
            }
        }

        let checker = ConsistencyChecker::new();
        let artifacts = checker
            .detect_seamline_artifacts(&buffer)
            .expect("seamline detection should succeed");

        assert!(
            !artifacts.is_empty(),
            "a genuine 10x-magnitude vertical seam at x=30 must be detected, not silently \
             reported as zero artifacts"
        );
        assert!(
            artifacts.iter().any(|a| a.start_x <= 30 && a.end_x >= 30),
            "detected seamline(s) should include the actual seam column (x=30), got: {artifacts:?}"
        );
    }

    #[test]
    fn test_detect_seamline_artifacts_reports_none_for_uniform_image() {
        // A perfectly uniform image has zero discontinuity everywhere, so
        // there is genuinely nothing to flag as an outlier.
        let buffer = RasterBuffer::zeros(50, 50, RasterDataType::Float32);
        let checker = ConsistencyChecker::new();
        let artifacts = checker
            .detect_seamline_artifacts(&buffer)
            .expect("seamline detection should succeed");
        assert!(artifacts.is_empty());
    }

    #[test]
    fn test_estimate_noise_level_samples_vary_across_the_image() {
        // Build an image whose left half is perfectly uniform (zero local
        // variance) and whose right half has strong per-pixel noise. If
        // estimate_noise_level always sampled the same fixed off-center
        // patch (the previous bug), this would be indistinguishable from an
        // image that is uniform (or noisy) everywhere. With real spatial
        // sampling, the measured noise level must be strictly greater than
        // sampling only the uniform half would produce.
        let width = 80u64;
        let height = 80u64;
        let mut noisy_buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);
        let mut uniform_buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

        for y in 0..height {
            for x in 0..width {
                uniform_buffer
                    .set_pixel(x, y, 42.0)
                    .expect("set_pixel should succeed");

                let value = if x < width / 2 {
                    42.0
                } else {
                    // Deterministic high-amplitude "noise" pattern.
                    42.0 + if (x + y) % 2 == 0 { 50.0 } else { -50.0 }
                };
                noisy_buffer
                    .set_pixel(x, y, value)
                    .expect("set_pixel should succeed");
            }
        }

        let checker = ConsistencyChecker::new();
        let uniform_noise = checker
            .estimate_noise_level(&uniform_buffer)
            .expect("noise estimation should succeed");
        let mixed_noise = checker
            .estimate_noise_level(&noisy_buffer)
            .expect("noise estimation should succeed");

        assert_eq!(
            uniform_noise, 0.0,
            "a perfectly uniform image must measure zero noise"
        );
        assert!(
            mixed_noise > uniform_noise,
            "an image with a genuinely noisy half must measure more noise than a uniform image; \
             got mixed={mixed_noise}, uniform={uniform_noise} -- if sampling always hit the same \
             fixed patch, these could be indistinguishable depending on which half that patch \
             fell in"
        );
    }
}

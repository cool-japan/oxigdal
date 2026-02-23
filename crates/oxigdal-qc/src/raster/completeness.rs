//! Raster data completeness checks.
//!
//! This module provides quality control checks for raster data completeness,
//! including NoData coverage analysis, gap detection, and band completeness.

use crate::error::{QcError, QcIssue, QcResult, Severity};
use oxigdal_core::buffer::RasterBuffer;

/// Result of raster completeness analysis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompletenessResult {
    /// Total number of pixels.
    pub total_pixels: u64,

    /// Number of valid (non-NoData) pixels.
    pub valid_pixels: u64,

    /// Number of NoData pixels.
    pub nodata_pixels: u64,

    /// Percentage of valid data (0.0 - 100.0).
    pub valid_percentage: f64,

    /// Number of detected gaps.
    pub gap_count: usize,

    /// Detected gaps information.
    pub gaps: Vec<GapInfo>,

    /// Number of bands checked.
    pub band_count: usize,

    /// Band completeness information.
    pub bands: Vec<BandCompleteness>,

    /// Quality control issues found.
    pub issues: Vec<QcIssue>,
}

/// Information about a detected gap in raster data.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GapInfo {
    /// Minimum X coordinate of the gap.
    pub min_x: u64,

    /// Minimum Y coordinate of the gap.
    pub min_y: u64,

    /// Maximum X coordinate of the gap.
    pub max_x: u64,

    /// Maximum Y coordinate of the gap.
    pub max_y: u64,

    /// Width of the gap in pixels.
    pub width: u64,

    /// Height of the gap in pixels.
    pub height: u64,

    /// Number of pixels in the gap.
    pub pixel_count: u64,

    /// Gap severity based on size.
    pub severity: Severity,
}

impl GapInfo {
    /// Creates a new gap information.
    #[must_use]
    pub fn new(min_x: u64, min_y: u64, max_x: u64, max_y: u64) -> Self {
        let width = max_x.saturating_sub(min_x).saturating_add(1);
        let height = max_y.saturating_sub(min_y).saturating_add(1);
        let pixel_count = width.saturating_mul(height);

        // Determine severity based on gap size
        let severity = if pixel_count > 10000 {
            Severity::Critical
        } else if pixel_count > 1000 {
            Severity::Major
        } else if pixel_count > 100 {
            Severity::Minor
        } else {
            Severity::Warning
        };

        Self {
            min_x,
            min_y,
            max_x,
            max_y,
            width,
            height,
            pixel_count,
            severity,
        }
    }
}

/// Band completeness information.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BandCompleteness {
    /// Band index (0-based).
    pub band_index: usize,

    /// Number of valid pixels in the band.
    pub valid_pixels: u64,

    /// Number of NoData pixels in the band.
    pub nodata_pixels: u64,

    /// Percentage of valid data (0.0 - 100.0).
    pub valid_percentage: f64,

    /// Whether the band meets minimum completeness threshold.
    pub meets_threshold: bool,
}

/// Configuration for completeness checks.
#[derive(Debug, Clone)]
pub struct CompletenessConfig {
    /// Minimum valid data percentage threshold (0.0 - 100.0).
    pub min_valid_percentage: f64,

    /// Maximum NoData percentage threshold (0.0 - 100.0).
    pub max_nodata_percentage: f64,

    /// Maximum gap size in pixels before flagging as issue.
    pub max_gap_size: u64,

    /// Whether to detect and report individual gaps.
    pub detect_gaps: bool,

    /// Whether to check band-by-band completeness.
    pub check_per_band: bool,
}

impl Default for CompletenessConfig {
    fn default() -> Self {
        Self {
            min_valid_percentage: 80.0,
            max_nodata_percentage: 20.0,
            max_gap_size: 100,
            detect_gaps: true,
            check_per_band: true,
        }
    }
}

/// Raster completeness checker.
pub struct CompletenessChecker {
    config: CompletenessConfig,
}

impl CompletenessChecker {
    /// Creates a new completeness checker with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: CompletenessConfig::default(),
        }
    }

    /// Creates a new completeness checker with custom configuration.
    #[must_use]
    pub fn with_config(config: CompletenessConfig) -> Self {
        Self { config }
    }

    /// Checks completeness of a single raster buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if the buffer cannot be analyzed.
    pub fn check_buffer(&self, buffer: &RasterBuffer) -> QcResult<CompletenessResult> {
        let mut issues = Vec::new();
        let total_pixels = buffer.pixel_count();
        let mut valid_pixels = 0u64;
        let mut nodata_pixels = 0u64;

        // Count valid and NoData pixels
        for y in 0..buffer.height() {
            for x in 0..buffer.width() {
                let value = buffer.get_pixel(x, y)?;
                if buffer.is_nodata(value) || !value.is_finite() {
                    nodata_pixels = nodata_pixels.saturating_add(1);
                } else {
                    valid_pixels = valid_pixels.saturating_add(1);
                }
            }
        }

        let valid_percentage = if total_pixels > 0 {
            (valid_pixels as f64 / total_pixels as f64) * 100.0
        } else {
            0.0
        };

        // Check against thresholds
        if valid_percentage < self.config.min_valid_percentage {
            issues.push(
                QcIssue::new(
                    Severity::Major,
                    "completeness",
                    "Insufficient valid data",
                    format!(
                        "Valid data percentage ({:.2}%) is below threshold ({:.2}%)",
                        valid_percentage, self.config.min_valid_percentage
                    ),
                )
                .with_suggestion("Review data source and processing pipeline"),
            );
        }

        let nodata_percentage = if total_pixels > 0 {
            (nodata_pixels as f64 / total_pixels as f64) * 100.0
        } else {
            0.0
        };

        if nodata_percentage > self.config.max_nodata_percentage {
            issues.push(
                QcIssue::new(
                    Severity::Warning,
                    "completeness",
                    "High NoData coverage",
                    format!(
                        "NoData percentage ({:.2}%) exceeds threshold ({:.2}%)",
                        nodata_percentage, self.config.max_nodata_percentage
                    ),
                )
                .with_suggestion("Investigate cause of missing data"),
            );
        }

        // Detect gaps if enabled
        let gaps = if self.config.detect_gaps {
            self.detect_gaps(buffer)?
        } else {
            Vec::new()
        };

        // Add issues for large gaps
        for gap in &gaps {
            if gap.pixel_count > self.config.max_gap_size {
                issues.push(
                    QcIssue::new(
                        gap.severity,
                        "completeness",
                        "Large data gap detected",
                        format!(
                            "Gap of {} pixels at ({}, {}) to ({}, {})",
                            gap.pixel_count, gap.min_x, gap.min_y, gap.max_x, gap.max_y
                        ),
                    )
                    .with_location(format!(
                        "({},{}) - ({},{})",
                        gap.min_x, gap.min_y, gap.max_x, gap.max_y
                    ))
                    .with_suggestion("Fill gap or mark as expected missing data"),
                );
            }
        }

        Ok(CompletenessResult {
            total_pixels,
            valid_pixels,
            nodata_pixels,
            valid_percentage,
            gap_count: gaps.len(),
            gaps,
            band_count: 1,
            bands: vec![BandCompleteness {
                band_index: 0,
                valid_pixels,
                nodata_pixels,
                valid_percentage,
                meets_threshold: valid_percentage >= self.config.min_valid_percentage,
            }],
            issues,
        })
    }

    /// Checks completeness of multiple bands.
    ///
    /// # Errors
    ///
    /// Returns an error if any buffer cannot be analyzed.
    pub fn check_bands(&self, bands: &[RasterBuffer]) -> QcResult<CompletenessResult> {
        if bands.is_empty() {
            return Err(QcError::InvalidInput("No bands provided".to_string()));
        }

        let mut total_pixels = 0u64;
        let mut valid_pixels = 0u64;
        let mut nodata_pixels = 0u64;
        let mut all_gaps = Vec::new();
        let mut band_completeness = Vec::new();
        let mut issues = Vec::new();

        // Check each band
        for (index, band) in bands.iter().enumerate() {
            let band_result = self.check_buffer(band)?;

            total_pixels = total_pixels.saturating_add(band_result.total_pixels);
            valid_pixels = valid_pixels.saturating_add(band_result.valid_pixels);
            nodata_pixels = nodata_pixels.saturating_add(band_result.nodata_pixels);

            // Add band-specific gaps with band index
            all_gaps.extend(band_result.gaps);

            band_completeness.push(BandCompleteness {
                band_index: index,
                valid_pixels: band_result.valid_pixels,
                nodata_pixels: band_result.nodata_pixels,
                valid_percentage: band_result.valid_percentage,
                meets_threshold: band_result.valid_percentage >= self.config.min_valid_percentage,
            });

            // Add band-specific issues
            for issue in band_result.issues {
                let mut band_issue = issue;
                band_issue.location = Some(format!("Band {}", index));
                issues.push(band_issue);
            }
        }

        let valid_percentage = if total_pixels > 0 {
            (valid_pixels as f64 / total_pixels as f64) * 100.0
        } else {
            0.0
        };

        // Check for inconsistent band completeness
        let completeness_variance = self.calculate_band_variance(&band_completeness);
        if completeness_variance > 10.0 {
            issues.push(
                QcIssue::new(
                    Severity::Warning,
                    "completeness",
                    "Inconsistent band completeness",
                    format!(
                        "Band completeness variance ({:.2}%) suggests inconsistent quality",
                        completeness_variance
                    ),
                )
                .with_suggestion("Review per-band processing and ensure consistent coverage"),
            );
        }

        Ok(CompletenessResult {
            total_pixels,
            valid_pixels,
            nodata_pixels,
            valid_percentage,
            gap_count: all_gaps.len(),
            gaps: all_gaps,
            band_count: bands.len(),
            bands: band_completeness,
            issues,
        })
    }

    /// Detects gaps in raster data using connected component analysis.
    ///
    /// # Errors
    ///
    /// Returns an error if gap detection fails.
    fn detect_gaps(&self, buffer: &RasterBuffer) -> QcResult<Vec<GapInfo>> {
        let width = buffer.width();
        let height = buffer.height();
        let mut visited = vec![vec![false; width as usize]; height as usize];
        let mut gaps = Vec::new();

        // Find connected NoData regions
        for y in 0..height {
            for x in 0..width {
                let value = buffer.get_pixel(x, y)?;
                if (buffer.is_nodata(value) || !value.is_finite())
                    && !visited[y as usize][x as usize]
                {
                    let gap = self.flood_fill_gap(buffer, x, y, &mut visited)?;
                    if gap.pixel_count > 1 {
                        // Only report gaps larger than single pixel
                        gaps.push(gap);
                    }
                }
            }
        }

        Ok(gaps)
    }

    /// Performs flood fill to identify a gap region.
    ///
    /// # Errors
    ///
    /// Returns an error if flood fill fails.
    fn flood_fill_gap(
        &self,
        buffer: &RasterBuffer,
        start_x: u64,
        start_y: u64,
        visited: &mut [Vec<bool>],
    ) -> QcResult<GapInfo> {
        let mut stack = vec![(start_x, start_y)];
        let mut min_x = start_x;
        let mut max_x = start_x;
        let mut min_y = start_y;
        let mut max_y = start_y;
        let mut pixel_count = 0u64;

        while let Some((x, y)) = stack.pop() {
            if x >= buffer.width() || y >= buffer.height() || visited[y as usize][x as usize] {
                continue;
            }

            let value = buffer.get_pixel(x, y)?;
            if !buffer.is_nodata(value) && value.is_finite() {
                continue;
            }

            visited[y as usize][x as usize] = true;
            pixel_count = pixel_count.saturating_add(1);

            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);

            // Add neighbors
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < buffer.width() {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < buffer.height() {
                stack.push((x, y + 1));
            }
        }

        Ok(GapInfo::new(min_x, min_y, max_x, max_y))
    }

    /// Calculates variance in band completeness percentages.
    fn calculate_band_variance(&self, bands: &[BandCompleteness]) -> f64 {
        if bands.len() < 2 {
            return 0.0;
        }

        let mean: f64 = bands.iter().map(|b| b.valid_percentage).sum::<f64>() / bands.len() as f64;

        let variance: f64 = bands
            .iter()
            .map(|b| {
                let diff = b.valid_percentage - mean;
                diff * diff
            })
            .sum::<f64>()
            / bands.len() as f64;

        variance.sqrt()
    }
}

impl Default for CompletenessChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::{NoDataValue, RasterDataType};

    #[test]
    fn test_completeness_checker_full_data() {
        let buffer = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
        let checker = CompletenessChecker::new();
        let result = checker.check_buffer(&buffer);

        assert!(result.is_ok());
        #[allow(clippy::unwrap_used)]
        let result = result.expect("completeness check should succeed for full data buffer");
        assert_eq!(result.total_pixels, 10000);
        assert_eq!(result.valid_pixels, 10000);
        assert_eq!(result.nodata_pixels, 0);
        assert!((result.valid_percentage - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_completeness_checker_with_nodata() {
        let buffer = RasterBuffer::nodata_filled(
            100,
            100,
            RasterDataType::Float32,
            NoDataValue::Float(-9999.0),
        );
        let checker = CompletenessChecker::new();
        let result = checker.check_buffer(&buffer);

        assert!(result.is_ok());
        #[allow(clippy::unwrap_used)]
        let result = result.expect("completeness check should succeed for nodata buffer");
        assert_eq!(result.total_pixels, 10000);
        assert_eq!(result.valid_pixels, 0);
        assert_eq!(result.nodata_pixels, 10000);
        assert!((result.valid_percentage - 0.0).abs() < f64::EPSILON);
        assert!(!result.issues.is_empty()); // Should have issues due to low completeness
    }

    #[test]
    fn test_gap_info_creation() {
        let gap = GapInfo::new(10, 20, 15, 25);
        assert_eq!(gap.width, 6);
        assert_eq!(gap.height, 6);
        assert_eq!(gap.pixel_count, 36);
    }

    #[test]
    fn test_band_completeness() {
        let band1 = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
        let band2 = RasterBuffer::nodata_filled(
            100,
            100,
            RasterDataType::Float32,
            NoDataValue::Float(-9999.0),
        );

        let checker = CompletenessChecker::new();
        let result = checker.check_bands(&[band1, band2]);

        assert!(result.is_ok());
        #[allow(clippy::unwrap_used)]
        let result = result.expect("band completeness check should succeed for multiple bands");
        assert_eq!(result.band_count, 2);
        assert_eq!(result.bands.len(), 2);
        assert!(result.bands[0].meets_threshold);
        assert!(!result.bands[1].meets_threshold);
    }

    #[test]
    fn test_custom_config() {
        let config = CompletenessConfig {
            min_valid_percentage: 50.0,
            max_nodata_percentage: 50.0,
            max_gap_size: 10,
            detect_gaps: false,
            check_per_band: true,
        };

        let checker = CompletenessChecker::with_config(config);
        let buffer = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
        let result = checker.check_buffer(&buffer);

        assert!(result.is_ok());
        #[allow(clippy::unwrap_used)]
        let result = result.expect("completeness check should succeed with custom config");
        assert_eq!(result.gap_count, 0); // Gap detection disabled
    }
}

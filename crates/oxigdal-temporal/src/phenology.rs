//! Vegetation Phenology Module
//!
//! Extracts phenological metrics from time series data including growing season
//! detection, peak timing, and amplitude calculations.

use crate::error::{Result, TemporalError};
use crate::timeseries::TimeSeriesRaster;
use chrono::Datelike;
use scirs2_core::ndarray::Array3;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Phenology extraction method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhenologyMethod {
    /// NDVI-based phenology
    NDVI,
    /// EVI-based phenology
    EVI,
    /// Threshold-based detection
    Threshold,
    /// Derivative-based detection
    Derivative,
}

/// Phenological metrics
#[derive(Debug, Clone)]
pub struct PhenologyMetrics {
    /// Start of growing season (day of year)
    pub season_start: Array3<i32>,
    /// End of growing season (day of year)
    pub season_end: Array3<i32>,
    /// Peak vegetation time (day of year)
    pub peak_time: Array3<i32>,
    /// Maximum vegetation value
    pub peak_value: Array3<f64>,
    /// Amplitude (peak - base)
    pub amplitude: Array3<f64>,
    /// Growing season length (days)
    pub season_length: Array3<i32>,
}

impl PhenologyMetrics {
    /// Create new phenology metrics
    #[must_use]
    pub fn new(
        season_start: Array3<i32>,
        season_end: Array3<i32>,
        peak_time: Array3<i32>,
        peak_value: Array3<f64>,
        amplitude: Array3<f64>,
        season_length: Array3<i32>,
    ) -> Self {
        Self {
            season_start,
            season_end,
            peak_time,
            peak_value,
            amplitude,
            season_length,
        }
    }
}

/// Phenology configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhenologyConfig {
    /// Method to use
    pub method: PhenologyMethod,
    /// Threshold for season detection (0-1)
    pub threshold: f64,
    /// Minimum season length (days)
    pub min_season_length: i32,
    /// Smoothing window size
    pub smoothing_window: Option<usize>,
}

impl Default for PhenologyConfig {
    fn default() -> Self {
        Self {
            method: PhenologyMethod::NDVI,
            threshold: 0.3,
            min_season_length: 60,
            smoothing_window: Some(3),
        }
    }
}

/// Phenology extractor
pub struct PhenologyExtractor;

impl PhenologyExtractor {
    /// Extract phenological metrics
    ///
    /// # Errors
    /// Returns error if extraction fails
    pub fn extract(ts: &TimeSeriesRaster, config: &PhenologyConfig) -> Result<PhenologyMetrics> {
        if ts.len() < 10 {
            return Err(TemporalError::insufficient_data(
                "Need at least 10 observations for phenology",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut season_start = Array3::from_elem((height, width, n_bands), -1);
        let mut season_end = Array3::from_elem((height, width, n_bands), -1);
        let mut peak_time = Array3::from_elem((height, width, n_bands), -1);
        let mut peak_value = Array3::zeros((height, width, n_bands));
        let mut amplitude = Array3::zeros((height, width, n_bands));
        let mut season_length = Array3::zeros((height, width, n_bands));

        // Collect day of year for each observation
        let doys: Vec<i32> = ts
            .iter()
            .map(|(_, e)| e.metadata.acquisition_date.ordinal() as i32)
            .collect();

        // Process each pixel
        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    // Apply smoothing if configured
                    let smoothed = if let Some(window) = config.smoothing_window {
                        Self::smooth_values(&values, window)
                    } else {
                        values.clone()
                    };

                    // Find min and max
                    let min_val = smoothed
                        .iter()
                        .copied()
                        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                        .unwrap_or(0.0);
                    let max_val = smoothed
                        .iter()
                        .copied()
                        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                        .unwrap_or(0.0);

                    let amp = max_val - min_val;
                    amplitude[[i, j, k]] = amp;

                    if amp < 0.01 {
                        // No significant seasonality
                        continue;
                    }

                    // Calculate threshold value
                    let threshold_val = min_val + config.threshold * amp;

                    // Find peak
                    if let Some((peak_idx, &peak)) =
                        smoothed.iter().enumerate().max_by(|(_, a), (_, b)| {
                            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                        })
                    {
                        peak_time[[i, j, k]] = doys[peak_idx];
                        peak_value[[i, j, k]] = peak;

                        // Find season start (ascending through threshold before peak)
                        for idx in 0..peak_idx {
                            if smoothed[idx] >= threshold_val {
                                season_start[[i, j, k]] = doys[idx];
                                break;
                            }
                        }

                        // Find season end (descending through threshold after peak)
                        for idx in (peak_idx + 1)..smoothed.len() {
                            if smoothed[idx] <= threshold_val {
                                season_end[[i, j, k]] = doys[idx];
                                break;
                            }
                        }

                        // Calculate season length
                        if season_start[[i, j, k]] >= 0 && season_end[[i, j, k]] >= 0 {
                            let length = season_end[[i, j, k]] - season_start[[i, j, k]];
                            if length >= config.min_season_length {
                                season_length[[i, j, k]] = length;
                            } else {
                                // Reset if season too short
                                season_start[[i, j, k]] = -1;
                                season_end[[i, j, k]] = -1;
                            }
                        }
                    }
                }
            }
        }

        info!("Extracted phenology metrics");
        Ok(PhenologyMetrics::new(
            season_start,
            season_end,
            peak_time,
            peak_value,
            amplitude,
            season_length,
        ))
    }

    /// Simple moving average smoothing
    fn smooth_values(values: &[f64], window: usize) -> Vec<f64> {
        let mut smoothed = Vec::with_capacity(values.len());

        for i in 0..values.len() {
            let start = i.saturating_sub(window / 2);
            let end = (i + window / 2 + 1).min(values.len());
            let window_values = &values[start..end];
            let avg = window_values.iter().sum::<f64>() / window_values.len() as f64;
            smoothed.push(avg);
        }

        smoothed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeseries::{TemporalMetadata, TimeSeriesRaster};
    use chrono::{DateTime, NaiveDate};

    #[test]
    fn test_phenology_extraction() {
        let mut ts = TimeSeriesRaster::new();

        // Simulate growing season (sinusoidal pattern)
        for i in 0..365 {
            if i % 10 == 0 {
                // Sample every 10 days
                let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
                let date =
                    NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid") + chrono::Duration::days(i);
                let metadata = TemporalMetadata::new(dt, date);

                // Sinusoidal pattern: peak around day 180
                let angle = (i as f64 / 365.0) * 2.0 * std::f64::consts::PI;
                let value = 0.3 + 0.4 * angle.sin(); // NDVI-like values 0.3-0.7

                let data = Array3::from_elem((5, 5, 1), value);
                ts.add_raster(metadata, data).expect("should add");
            }
        }

        let config = PhenologyConfig::default();
        let metrics = PhenologyExtractor::extract(&ts, &config).expect("should extract");

        // Check that metrics were computed
        assert!(metrics.season_start[[2, 2, 0]] > 0);
        assert!(metrics.peak_time[[2, 2, 0]] > 0);
        assert!(metrics.amplitude[[2, 2, 0]] > 0.0);
    }
}

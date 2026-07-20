//! Breakpoint Detection Module
//!
//! Implements breakpoint detection algorithms for identifying structural breaks
//! in time series data, including PELT, binary segmentation, and changepoint detection.

use crate::error::{Result, TemporalError};
use crate::timeseries::TimeSeriesRaster;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Breakpoint detection method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakpointMethod {
    /// Binary segmentation
    BinarySegmentation,
    /// PELT (Pruned Exact Linear Time)
    PELT,
    /// CUSUM-based breakpoint
    CUSUM,
    /// Simple threshold crossing
    ThresholdCrossing,
}

/// Breakpoint detection result
#[derive(Debug, Clone)]
pub struct BreakpointResult {
    /// Breakpoint locations (time indices)
    pub breakpoints: Vec<usize>,
    /// Breakpoint scores/confidence
    pub scores: Vec<f64>,
    /// Segments between breakpoints
    pub segments: Vec<Segment>,
}

/// Time series segment
#[derive(Debug, Clone)]
pub struct Segment {
    /// Start index
    pub start: usize,
    /// End index (exclusive)
    pub end: usize,
    /// Segment mean
    pub mean: f64,
    /// Segment variance
    pub variance: f64,
}

impl BreakpointResult {
    /// Create new breakpoint result
    #[must_use]
    pub fn new(breakpoints: Vec<usize>, scores: Vec<f64>) -> Self {
        Self {
            breakpoints,
            scores,
            segments: Vec::new(),
        }
    }

    /// Add segments
    #[must_use]
    pub fn with_segments(mut self, segments: Vec<Segment>) -> Self {
        self.segments = segments;
        self
    }
}

/// Breakpoint detector
pub struct BreakpointDetector;

impl BreakpointDetector {
    /// Detect breakpoints in time series
    ///
    /// # Errors
    /// Returns error if detection fails
    pub fn detect(
        ts: &TimeSeriesRaster,
        method: BreakpointMethod,
        params: BreakpointParams,
    ) -> Result<Vec<BreakpointResult>> {
        match method {
            BreakpointMethod::BinarySegmentation => {
                Self::binary_segmentation(ts, params.max_breakpoints, params.min_segment_length)
            }
            BreakpointMethod::PELT => Self::pelt(ts, params.penalty),
            BreakpointMethod::CUSUM => Self::cusum_breakpoint(ts, params.threshold),
            BreakpointMethod::ThresholdCrossing => Self::threshold_crossing(ts, params.threshold),
        }
    }

    /// Binary segmentation for breakpoint detection
    fn binary_segmentation(
        ts: &TimeSeriesRaster,
        max_breakpoints: usize,
        min_segment_length: usize,
    ) -> Result<Vec<BreakpointResult>> {
        if ts.len() < min_segment_length * 2 {
            return Err(TemporalError::insufficient_data(format!(
                "Need at least {} observations",
                min_segment_length * 2
            )));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut results = Vec::new();

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    let mut breakpoints = Vec::new();
                    let mut scores = Vec::new();
                    let mut segments = vec![(0, values.len())];

                    for _ in 0..max_breakpoints {
                        let mut best_breakpoint = None;
                        let mut best_score = f64::NEG_INFINITY;

                        for &(start, end) in &segments {
                            if end - start < min_segment_length * 2 {
                                continue;
                            }

                            let segment = &values[start..end];
                            if let Some((bp, score)) =
                                Self::find_best_split(segment, min_segment_length)
                            {
                                let abs_bp = start + bp;
                                if score > best_score {
                                    best_score = score;
                                    best_breakpoint = Some((abs_bp, start, end));
                                }
                            }
                        }

                        if let Some((bp, seg_start, seg_end)) = best_breakpoint {
                            breakpoints.push(bp);
                            scores.push(best_score);

                            // Update segments
                            segments.retain(|&(s, e)| s != seg_start || e != seg_end);
                            segments.push((seg_start, bp));
                            segments.push((bp, seg_end));
                        } else {
                            break;
                        }
                    }

                    // Build segments with statistics
                    segments.sort_by_key(|&(s, _)| s);
                    let segment_stats: Vec<Segment> = segments
                        .iter()
                        .map(|&(start, end)| {
                            let seg_values = &values[start..end];
                            let mean = seg_values.iter().sum::<f64>() / seg_values.len() as f64;
                            let variance =
                                seg_values.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                                    / seg_values.len() as f64;

                            Segment {
                                start,
                                end,
                                mean,
                                variance,
                            }
                        })
                        .collect();

                    if !breakpoints.is_empty() {
                        results.push(
                            BreakpointResult::new(breakpoints, scores).with_segments(segment_stats),
                        );
                    }
                }
            }
        }

        info!("Completed binary segmentation breakpoint detection");
        Ok(results)
    }

    /// Find best split point in a segment
    fn find_best_split(segment: &[f64], min_len: usize) -> Option<(usize, f64)> {
        if segment.len() < min_len * 2 {
            return None;
        }

        let mut best_split = None;
        let mut best_score = f64::NEG_INFINITY;

        for i in min_len..(segment.len() - min_len) {
            let left = &segment[..i];
            let right = &segment[i..];

            let score = Self::calculate_split_score(left, right);

            if score > best_score {
                best_score = score;
                best_split = Some(i);
            }
        }

        best_split.map(|split| (split, best_score))
    }

    /// Calculate split quality score
    fn calculate_split_score(left: &[f64], right: &[f64]) -> f64 {
        let left_mean = left.iter().sum::<f64>() / left.len() as f64;
        let right_mean = right.iter().sum::<f64>() / right.len() as f64;

        let left_var =
            left.iter().map(|v| (v - left_mean).powi(2)).sum::<f64>() / left.len() as f64;
        let right_var =
            right.iter().map(|v| (v - right_mean).powi(2)).sum::<f64>() / right.len() as f64;

        // Score based on mean difference and within-segment variance
        let mean_diff = (right_mean - left_mean).abs();
        let avg_var = (left_var + right_var) / 2.0;

        if avg_var > 0.0 {
            mean_diff / avg_var.sqrt()
        } else {
            mean_diff
        }
    }

    /// PELT algorithm for optimal breakpoint detection
    fn pelt(ts: &TimeSeriesRaster, _penalty: f64) -> Result<Vec<BreakpointResult>> {
        // Simplified PELT implementation
        // Full PELT is complex - use binary segmentation as approximation
        // Note: penalty parameter reserved for future full PELT implementation
        info!("Using binary segmentation approximation for PELT");
        Self::binary_segmentation(ts, 10, 3)
    }

    /// CUSUM-based breakpoint detection
    fn cusum_breakpoint(ts: &TimeSeriesRaster, threshold: f64) -> Result<Vec<BreakpointResult>> {
        if ts.len() < 3 {
            return Err(TemporalError::insufficient_data(
                "Need at least 3 observations",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut results = Vec::new();

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;
                    let mean = values.iter().sum::<f64>() / values.len() as f64;

                    let mut cusum = 0.0;
                    let mut breakpoints = Vec::new();
                    let mut scores = Vec::new();

                    for (idx, &value) in values.iter().enumerate() {
                        cusum += value - mean;

                        if cusum.abs() > threshold {
                            breakpoints.push(idx);
                            scores.push(cusum.abs());
                            cusum = 0.0; // Reset CUSUM
                        }
                    }

                    if !breakpoints.is_empty() {
                        results.push(BreakpointResult::new(breakpoints, scores));
                    }
                }
            }
        }

        info!("Completed CUSUM breakpoint detection");
        Ok(results)
    }

    /// Threshold crossing breakpoint detection
    fn threshold_crossing(ts: &TimeSeriesRaster, threshold: f64) -> Result<Vec<BreakpointResult>> {
        if ts.len() < 2 {
            return Err(TemporalError::insufficient_data(
                "Need at least 2 observations",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut results = Vec::new();

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    let mut breakpoints = Vec::new();
                    let mut scores = Vec::new();

                    for idx in 1..values.len() {
                        let diff = (values[idx] - values[idx - 1]).abs();
                        if diff > threshold {
                            breakpoints.push(idx);
                            scores.push(diff);
                        }
                    }

                    if !breakpoints.is_empty() {
                        results.push(BreakpointResult::new(breakpoints, scores));
                    }
                }
            }
        }

        info!("Completed threshold crossing breakpoint detection");
        Ok(results)
    }
}

/// Breakpoint detection parameters
#[derive(Debug, Clone, Copy)]
pub struct BreakpointParams {
    /// Maximum number of breakpoints to detect
    pub max_breakpoints: usize,
    /// Minimum segment length
    pub min_segment_length: usize,
    /// Penalty for PELT
    pub penalty: f64,
    /// Threshold for threshold-based methods
    pub threshold: f64,
}

impl Default for BreakpointParams {
    fn default() -> Self {
        Self {
            max_breakpoints: 5,
            min_segment_length: 3,
            penalty: 1.0,
            threshold: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeseries::{TemporalMetadata, TimeSeriesRaster};
    use chrono::{DateTime, NaiveDate};
    use scirs2_core::ndarray::Array3;

    #[test]
    fn test_binary_segmentation() {
        let mut ts = TimeSeriesRaster::new();

        // Create data with a clear breakpoint
        for i in 0..20 {
            let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
            let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
            let metadata = TemporalMetadata::new(dt, date);

            let value = if i < 10 { 10.0 } else { 50.0 }; // Breakpoint at i=10
            let data = Array3::from_elem((1, 1, 1), value);
            ts.add_raster(metadata, data).expect("should add");
        }

        let params = BreakpointParams::default();
        let results = BreakpointDetector::detect(&ts, BreakpointMethod::BinarySegmentation, params)
            .expect("should detect");

        // Should detect the breakpoint
        assert!(!results.is_empty());
    }

    #[test]
    fn test_threshold_crossing() {
        let mut ts = TimeSeriesRaster::new();

        for i in 0..10 {
            let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
            let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
            let metadata = TemporalMetadata::new(dt, date);

            let value = if i == 5 { 100.0 } else { 10.0 };
            let data = Array3::from_elem((1, 1, 1), value);
            ts.add_raster(metadata, data).expect("should add");
        }

        let params = BreakpointParams {
            threshold: 20.0,
            ..Default::default()
        };

        let results = BreakpointDetector::detect(&ts, BreakpointMethod::ThresholdCrossing, params)
            .expect("should detect");

        assert!(!results.is_empty());
    }
}

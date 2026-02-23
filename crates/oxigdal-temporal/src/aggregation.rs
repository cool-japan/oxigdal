//! Temporal Aggregation Module
//!
//! This module provides methods for aggregating time series data over various temporal windows,
//! including daily, weekly, monthly, yearly, and rolling aggregations.

use crate::error::{Result, TemporalError};
use crate::timeseries::{TemporalMetadata, TimeSeriesRaster};
use chrono::{DateTime, Datelike, Duration, Utc};
use scirs2_core::ndarray::Array3;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};

/// Type alias for temporal groups
type TemporalGroups<'a> = HashMap<String, Vec<(DateTime<Utc>, &'a Array3<f64>)>>;

/// Temporal window for aggregation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemporalWindow {
    /// Daily aggregation
    Daily,
    /// Weekly aggregation (7 days)
    Weekly,
    /// Monthly aggregation
    Monthly,
    /// Yearly aggregation
    Yearly,
    /// Custom number of days
    CustomDays(i64),
    /// Rolling window (number of observations)
    Rolling(usize),
}

/// Aggregation statistic
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregationStatistic {
    /// Mean value
    Mean,
    /// Median value
    Median,
    /// Minimum value
    Min,
    /// Maximum value
    Max,
    /// Sum
    Sum,
    /// Standard deviation
    StdDev,
    /// Count of valid observations
    Count,
    /// First value
    First,
    /// Last value
    Last,
}

/// Aggregation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationConfig {
    /// Temporal window
    pub window: TemporalWindow,
    /// Statistics to compute
    pub statistics: Vec<AggregationStatistic>,
    /// NoData value to ignore
    pub nodata: Option<f64>,
    /// Minimum number of valid observations required
    pub min_observations: usize,
}

impl Default for AggregationConfig {
    fn default() -> Self {
        Self {
            window: TemporalWindow::Monthly,
            statistics: vec![AggregationStatistic::Mean],
            nodata: Some(f64::NAN),
            min_observations: 1,
        }
    }
}

/// Result of temporal aggregation
#[derive(Debug, Clone)]
pub struct AggregationResult {
    /// Aggregated time series (one entry per window)
    pub time_series: HashMap<String, TimeSeriesRaster>,
    /// Window start times
    pub window_starts: Vec<DateTime<Utc>>,
    /// Window end times
    pub window_ends: Vec<DateTime<Utc>>,
}

impl AggregationResult {
    /// Create new aggregation result
    #[must_use]
    pub fn new() -> Self {
        Self {
            time_series: HashMap::new(),
            window_starts: Vec::new(),
            window_ends: Vec::new(),
        }
    }

    /// Get time series for specific statistic
    #[must_use]
    pub fn get(&self, stat: &str) -> Option<&TimeSeriesRaster> {
        self.time_series.get(stat)
    }

    /// Add time series for statistic
    pub fn add(&mut self, stat: String, ts: TimeSeriesRaster) {
        self.time_series.insert(stat, ts);
    }
}

impl Default for AggregationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Temporal aggregator
pub struct TemporalAggregator;

impl TemporalAggregator {
    /// Aggregate time series data
    ///
    /// # Errors
    /// Returns error if aggregation fails
    pub fn aggregate(
        ts: &TimeSeriesRaster,
        config: &AggregationConfig,
    ) -> Result<AggregationResult> {
        match config.window {
            TemporalWindow::Daily => Self::aggregate_daily(ts, config),
            TemporalWindow::Weekly => Self::aggregate_weekly(ts, config),
            TemporalWindow::Monthly => Self::aggregate_monthly(ts, config),
            TemporalWindow::Yearly => Self::aggregate_yearly(ts, config),
            TemporalWindow::CustomDays(days) => Self::aggregate_custom_days(ts, config, days),
            TemporalWindow::Rolling(size) => Self::aggregate_rolling(ts, config, size),
        }
    }

    /// Aggregate to daily intervals
    fn aggregate_daily(
        ts: &TimeSeriesRaster,
        config: &AggregationConfig,
    ) -> Result<AggregationResult> {
        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        // Group by day
        let mut daily_groups: TemporalGroups = HashMap::new();

        for (_, entry) in ts.iter() {
            let date_key = entry.metadata.acquisition_date.to_string();
            if let Some(data) = entry.data.as_ref() {
                daily_groups
                    .entry(date_key)
                    .or_default()
                    .push((entry.metadata.timestamp, data));
            }
        }

        Self::compute_statistics(&daily_groups, config, height, width, n_bands)
    }

    /// Aggregate to weekly intervals
    fn aggregate_weekly(
        ts: &TimeSeriesRaster,
        config: &AggregationConfig,
    ) -> Result<AggregationResult> {
        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        // Group by ISO week
        let mut weekly_groups: TemporalGroups = HashMap::new();

        for (_, entry) in ts.iter() {
            let year = entry.metadata.acquisition_date.year();
            let week = entry.metadata.acquisition_date.iso_week().week();
            let week_key = format!("{}-W{:02}", year, week);

            if let Some(data) = entry.data.as_ref() {
                weekly_groups
                    .entry(week_key)
                    .or_default()
                    .push((entry.metadata.timestamp, data));
            }
        }

        Self::compute_statistics(&weekly_groups, config, height, width, n_bands)
    }

    /// Aggregate to monthly intervals
    fn aggregate_monthly(
        ts: &TimeSeriesRaster,
        config: &AggregationConfig,
    ) -> Result<AggregationResult> {
        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        // Group by year-month
        let mut monthly_groups: TemporalGroups = HashMap::new();

        for (_, entry) in ts.iter() {
            let year = entry.metadata.acquisition_date.year();
            let month = entry.metadata.acquisition_date.month();
            let month_key = format!("{}-{:02}", year, month);

            if let Some(data) = entry.data.as_ref() {
                monthly_groups
                    .entry(month_key)
                    .or_default()
                    .push((entry.metadata.timestamp, data));
            }
        }

        Self::compute_statistics(&monthly_groups, config, height, width, n_bands)
    }

    /// Aggregate to yearly intervals
    fn aggregate_yearly(
        ts: &TimeSeriesRaster,
        config: &AggregationConfig,
    ) -> Result<AggregationResult> {
        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        // Group by year
        let mut yearly_groups: TemporalGroups = HashMap::new();

        for (_, entry) in ts.iter() {
            let year = entry.metadata.acquisition_date.year();
            let year_key = format!("{}", year);

            if let Some(data) = entry.data.as_ref() {
                yearly_groups
                    .entry(year_key)
                    .or_default()
                    .push((entry.metadata.timestamp, data));
            }
        }

        Self::compute_statistics(&yearly_groups, config, height, width, n_bands)
    }

    /// Aggregate to custom day intervals
    fn aggregate_custom_days(
        ts: &TimeSeriesRaster,
        config: &AggregationConfig,
        days: i64,
    ) -> Result<AggregationResult> {
        if days <= 0 {
            return Err(TemporalError::invalid_parameter("days", "must be positive"));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let (start_time, end_time) = ts
            .time_range()
            .ok_or_else(|| TemporalError::insufficient_data("Empty time series"))?;

        // Create windows
        let mut window_groups: TemporalGroups = HashMap::new();
        let mut current = start_time;

        while current < end_time {
            let next = current + Duration::days(days);
            let window_key = format!("{}", current.format("%Y-%m-%d"));

            // Get entries in this window
            let entries = ts.query_range(&current, &next);
            for entry in entries {
                if let Some(data) = entry.data.as_ref() {
                    window_groups
                        .entry(window_key.clone())
                        .or_default()
                        .push((entry.metadata.timestamp, data));
                }
            }

            current = next;
        }

        Self::compute_statistics(&window_groups, config, height, width, n_bands)
    }

    /// Rolling window aggregation
    fn aggregate_rolling(
        ts: &TimeSeriesRaster,
        config: &AggregationConfig,
        window_size: usize,
    ) -> Result<AggregationResult> {
        if window_size == 0 {
            return Err(TemporalError::invalid_parameter(
                "window_size",
                "must be greater than 0",
            ));
        }

        if window_size > ts.len() {
            return Err(TemporalError::invalid_parameter(
                "window_size",
                "exceeds time series length",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let entries: Vec<_> = ts.iter().collect();
        let mut result = AggregationResult::new();

        // Initialize time series for each statistic
        for stat in &config.statistics {
            result.add(format!("{:?}", stat), TimeSeriesRaster::new());
        }

        // Slide window through time series
        for i in 0..=(entries.len().saturating_sub(window_size)) {
            let window_entries: Vec<_> = entries[i..i + window_size]
                .iter()
                .filter_map(|(_, e)| e.data.as_ref().map(|d| (e.metadata.timestamp, d)))
                .collect();

            if window_entries.len() < config.min_observations {
                continue;
            }

            // Get window timestamp (middle of window)
            let mid_timestamp = window_entries[window_entries.len() / 2].0;

            // Compute each statistic
            for stat in &config.statistics {
                let aggregated = Self::compute_statistic(
                    &window_entries,
                    *stat,
                    config,
                    height,
                    width,
                    n_bands,
                )?;

                let metadata = TemporalMetadata::new(mid_timestamp, mid_timestamp.date_naive());
                let stat_key = format!("{:?}", stat);
                if let Some(ts) = result.time_series.get_mut(&stat_key) {
                    ts.add_raster(metadata, aggregated)?;
                }
            }
        }

        info!(
            "Completed rolling aggregation with window size {}",
            window_size
        );
        Ok(result)
    }

    /// Compute statistics for grouped data
    fn compute_statistics(
        groups: &TemporalGroups,
        config: &AggregationConfig,
        height: usize,
        width: usize,
        n_bands: usize,
    ) -> Result<AggregationResult> {
        let mut result = AggregationResult::new();

        // Initialize time series for each statistic
        for stat in &config.statistics {
            result.add(format!("{:?}", stat), TimeSeriesRaster::new());
        }

        // Process each group
        let mut sorted_keys: Vec<_> = groups.keys().collect();
        sorted_keys.sort();

        for key in sorted_keys {
            let group = &groups[key];

            if group.len() < config.min_observations {
                debug!(
                    "Skipping group {} with {} observations (min: {})",
                    key,
                    group.len(),
                    config.min_observations
                );
                continue;
            }

            // Get first timestamp as representative
            let timestamp = group[0].0;

            // Compute each statistic
            for stat in &config.statistics {
                let aggregated =
                    Self::compute_statistic(group, *stat, config, height, width, n_bands)?;

                let metadata = TemporalMetadata::new(timestamp, timestamp.date_naive());
                let stat_key = format!("{:?}", stat);
                if let Some(ts) = result.time_series.get_mut(&stat_key) {
                    ts.add_raster(metadata, aggregated)?;
                }
            }
        }

        info!(
            "Aggregated {} groups with {} statistics",
            groups.len(),
            config.statistics.len()
        );
        Ok(result)
    }

    /// Compute a single statistic
    fn compute_statistic(
        entries: &[(DateTime<Utc>, &Array3<f64>)],
        stat: AggregationStatistic,
        config: &AggregationConfig,
        height: usize,
        width: usize,
        n_bands: usize,
    ) -> Result<Array3<f64>> {
        let mut result: Array3<f64> = Array3::zeros((height, width, n_bands));

        match stat {
            AggregationStatistic::Mean => {
                let mut sum: Array3<f64> = Array3::zeros((height, width, n_bands));
                let mut count: Array3<f64> = Array3::zeros((height, width, n_bands));

                for (_, data) in entries {
                    for i in 0..height {
                        for j in 0..width {
                            for k in 0..n_bands {
                                let val = data[[i, j, k]];
                                if Self::is_valid(val, config.nodata) {
                                    sum[[i, j, k]] += val;
                                    count[[i, j, k]] += 1.0;
                                }
                            }
                        }
                    }
                }

                for i in 0..height {
                    for j in 0..width {
                        for k in 0..n_bands {
                            result[[i, j, k]] = if count[[i, j, k]] > 0.0 {
                                sum[[i, j, k]] / count[[i, j, k]]
                            } else {
                                config.nodata.unwrap_or(f64::NAN)
                            };
                        }
                    }
                }
            }
            AggregationStatistic::Min => {
                result.fill(f64::INFINITY);
                for (_, data) in entries {
                    for i in 0..height {
                        for j in 0..width {
                            for k in 0..n_bands {
                                let val = data[[i, j, k]];
                                if Self::is_valid(val, config.nodata) && val < result[[i, j, k]] {
                                    result[[i, j, k]] = val;
                                }
                            }
                        }
                    }
                }
                // Replace INFINITY with nodata
                for i in 0..height {
                    for j in 0..width {
                        for k in 0..n_bands {
                            if result[[i, j, k]].is_infinite() {
                                result[[i, j, k]] = config.nodata.unwrap_or(f64::NAN);
                            }
                        }
                    }
                }
            }
            AggregationStatistic::Max => {
                result.fill(f64::NEG_INFINITY);
                for (_, data) in entries {
                    for i in 0..height {
                        for j in 0..width {
                            for k in 0..n_bands {
                                let val = data[[i, j, k]];
                                if Self::is_valid(val, config.nodata) && val > result[[i, j, k]] {
                                    result[[i, j, k]] = val;
                                }
                            }
                        }
                    }
                }
                // Replace NEG_INFINITY with nodata
                for i in 0..height {
                    for j in 0..width {
                        for k in 0..n_bands {
                            if result[[i, j, k]].is_infinite() {
                                result[[i, j, k]] = config.nodata.unwrap_or(f64::NAN);
                            }
                        }
                    }
                }
            }
            AggregationStatistic::Sum => {
                for (_, data) in entries {
                    for i in 0..height {
                        for j in 0..width {
                            for k in 0..n_bands {
                                let val = data[[i, j, k]];
                                if Self::is_valid(val, config.nodata) {
                                    result[[i, j, k]] += val;
                                }
                            }
                        }
                    }
                }
            }
            AggregationStatistic::Count => {
                for (_, data) in entries {
                    for i in 0..height {
                        for j in 0..width {
                            for k in 0..n_bands {
                                let val = data[[i, j, k]];
                                if Self::is_valid(val, config.nodata) {
                                    result[[i, j, k]] += 1.0;
                                }
                            }
                        }
                    }
                }
            }
            AggregationStatistic::First => {
                if let Some((_, first_data)) = entries.first() {
                    result = (*first_data).clone();
                }
            }
            AggregationStatistic::Last => {
                if let Some((_, last_data)) = entries.last() {
                    result = (*last_data).clone();
                }
            }
            AggregationStatistic::StdDev => {
                // First pass: compute mean
                let mut mean: Array3<f64> = Array3::zeros((height, width, n_bands));
                let mut count: Array3<f64> = Array3::zeros((height, width, n_bands));

                for (_, data) in entries {
                    for i in 0..height {
                        for j in 0..width {
                            for k in 0..n_bands {
                                let val = data[[i, j, k]];
                                if Self::is_valid(val, config.nodata) {
                                    mean[[i, j, k]] += val;
                                    count[[i, j, k]] += 1.0;
                                }
                            }
                        }
                    }
                }

                for i in 0..height {
                    for j in 0..width {
                        for k in 0..n_bands {
                            if count[[i, j, k]] > 0.0 {
                                mean[[i, j, k]] /= count[[i, j, k]];
                            }
                        }
                    }
                }

                // Second pass: compute variance
                let mut variance: Array3<f64> = Array3::zeros((height, width, n_bands));

                for (_, data) in entries {
                    for i in 0..height {
                        for j in 0..width {
                            for k in 0..n_bands {
                                let val = data[[i, j, k]];
                                if Self::is_valid(val, config.nodata) {
                                    let diff = val - mean[[i, j, k]];
                                    variance[[i, j, k]] += diff * diff;
                                }
                            }
                        }
                    }
                }

                for i in 0..height {
                    for j in 0..width {
                        for k in 0..n_bands {
                            result[[i, j, k]] = if count[[i, j, k]] > 1.0 {
                                (variance[[i, j, k]] / count[[i, j, k]]).sqrt()
                            } else {
                                config.nodata.unwrap_or(f64::NAN)
                            };
                        }
                    }
                }
            }
            AggregationStatistic::Median => {
                // Collect values for each pixel
                let mut pixel_values: Vec<Vec<Vec<Vec<f64>>>> =
                    vec![vec![vec![Vec::new(); n_bands]; width]; height];

                for (_, data) in entries {
                    for i in 0..height {
                        for j in 0..width {
                            for k in 0..n_bands {
                                let val = data[[i, j, k]];
                                if Self::is_valid(val, config.nodata) {
                                    pixel_values[i][j][k].push(val);
                                }
                            }
                        }
                    }
                }

                // Compute median for each pixel
                for i in 0..height {
                    for j in 0..width {
                        for k in 0..n_bands {
                            let values = &mut pixel_values[i][j][k];
                            if values.is_empty() {
                                result[[i, j, k]] = config.nodata.unwrap_or(f64::NAN);
                            } else {
                                values.sort_by(|a, b| {
                                    a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                                });
                                let mid = values.len() / 2;
                                result[[i, j, k]] = if values.len() % 2 == 0 {
                                    (values[mid - 1] + values[mid]) / 2.0
                                } else {
                                    values[mid]
                                };
                            }
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    /// Check if value is valid (not nodata)
    fn is_valid(val: f64, nodata: Option<f64>) -> bool {
        if let Some(nd) = nodata {
            if nd.is_nan() {
                !val.is_nan()
            } else {
                (val - nd).abs() > 1e-10
            }
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeseries::TemporalMetadata;
    use chrono::NaiveDate;

    fn create_test_timeseries() -> TimeSeriesRaster {
        let mut ts = TimeSeriesRaster::new();

        for i in 0..30 {
            let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
            let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
            let metadata = TemporalMetadata::new(dt, date);
            let data = Array3::from_elem((5, 5, 2), i as f64);
            ts.add_raster(metadata, data).expect("should add");
        }

        ts
    }

    #[test]
    fn test_daily_aggregation() {
        let ts = create_test_timeseries();
        let config = AggregationConfig {
            window: TemporalWindow::Daily,
            statistics: vec![AggregationStatistic::Mean],
            ..Default::default()
        };

        let result = TemporalAggregator::aggregate(&ts, &config).expect("should aggregate");
        assert!(result.get("Mean").is_some());
    }

    #[test]
    fn test_weekly_aggregation() {
        let ts = create_test_timeseries();
        let config = AggregationConfig {
            window: TemporalWindow::Weekly,
            statistics: vec![AggregationStatistic::Mean, AggregationStatistic::Max],
            ..Default::default()
        };

        let result = TemporalAggregator::aggregate(&ts, &config).expect("should aggregate");
        assert!(result.get("Mean").is_some());
        assert!(result.get("Max").is_some());
    }

    #[test]
    fn test_monthly_aggregation() {
        let ts = create_test_timeseries();
        let config = AggregationConfig {
            window: TemporalWindow::Monthly,
            statistics: vec![AggregationStatistic::Mean],
            ..Default::default()
        };

        let result = TemporalAggregator::aggregate(&ts, &config).expect("should aggregate");
        let mean_ts = result.get("Mean").expect("should have mean");
        assert!(!mean_ts.is_empty());
    }

    #[test]
    fn test_rolling_aggregation() {
        let ts = create_test_timeseries();
        let config = AggregationConfig {
            window: TemporalWindow::Rolling(7),
            statistics: vec![AggregationStatistic::Mean],
            min_observations: 5,
            ..Default::default()
        };

        let result = TemporalAggregator::aggregate(&ts, &config).expect("should aggregate");
        let mean_ts = result.get("Mean").expect("should have mean");
        assert!(!mean_ts.is_empty());
    }

    #[test]
    fn test_multiple_statistics() {
        let ts = create_test_timeseries();
        let config = AggregationConfig {
            window: TemporalWindow::Weekly,
            statistics: vec![
                AggregationStatistic::Mean,
                AggregationStatistic::Min,
                AggregationStatistic::Max,
                AggregationStatistic::StdDev,
            ],
            ..Default::default()
        };

        let result = TemporalAggregator::aggregate(&ts, &config).expect("should aggregate");
        assert!(result.get("Mean").is_some());
        assert!(result.get("Min").is_some());
        assert!(result.get("Max").is_some());
        assert!(result.get("StdDev").is_some());
    }
}

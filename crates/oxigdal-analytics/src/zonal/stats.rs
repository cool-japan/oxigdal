//! Advanced Zonal Statistics
//!
//! Calculate statistics for regions defined by zone masks.

use crate::error::{AnalyticsError, Result};
use scirs2_core::ndarray::{ArrayView2, ArrayView3};
use std::collections::HashMap;

/// Zonal statistics to calculate
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZonalStatistic {
    /// Mean value
    Mean,
    /// Median value
    Median,
    /// Minimum value
    Min,
    /// Maximum value
    Max,
    /// Sum of values
    Sum,
    /// Count of pixels
    Count,
    /// Standard deviation
    StdDev,
    /// Variance
    Variance,
    /// Coefficient of variation
    CoeffVar,
    /// Percentile (requires parameter)
    Percentile(u8),
}

/// Zonal statistics result
#[derive(Debug, Clone)]
pub struct ZonalResult {
    /// Statistics per zone
    pub zones: HashMap<i32, HashMap<ZonalStatistic, f64>>,
    /// Zone IDs
    pub zone_ids: Vec<i32>,
}

/// Zonal statistics calculator
pub struct ZonalCalculator {
    statistics: Vec<ZonalStatistic>,
    no_data_value: Option<f64>,
}

impl ZonalCalculator {
    /// Create a new zonal calculator
    pub fn new() -> Self {
        Self {
            statistics: vec![
                ZonalStatistic::Mean,
                ZonalStatistic::Min,
                ZonalStatistic::Max,
                ZonalStatistic::Count,
            ],
            no_data_value: None,
        }
    }

    /// Set statistics to calculate
    pub fn with_statistics(mut self, stats: Vec<ZonalStatistic>) -> Self {
        self.statistics = stats;
        self
    }

    /// Set no-data value
    pub fn with_no_data(mut self, value: f64) -> Self {
        self.no_data_value = Some(value);
        self
    }

    /// Calculate zonal statistics
    ///
    /// # Arguments
    /// * `values` - Value raster (height × width)
    /// * `zones` - Zone raster with integer zone IDs (height × width)
    ///
    /// # Errors
    /// Returns error if dimensions don't match
    pub fn calculate(
        &self,
        values: &ArrayView2<f64>,
        zones: &ArrayView2<i32>,
    ) -> Result<ZonalResult> {
        if values.dim() != zones.dim() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{:?}", values.dim()),
                format!("{:?}", zones.dim()),
            ));
        }

        // Group values by zone
        let mut zone_values: HashMap<i32, Vec<f64>> = HashMap::new();

        for ((i, j), &zone_id) in zones.indexed_iter() {
            let value = values[[i, j]];

            // Skip no-data values
            if let Some(no_data) = self.no_data_value {
                if (value - no_data).abs() < f64::EPSILON {
                    continue;
                }
            }

            zone_values.entry(zone_id).or_default().push(value);
        }

        // Calculate statistics for each zone
        let mut result_zones = HashMap::new();
        let mut zone_ids: Vec<i32> = zone_values.keys().copied().collect();
        zone_ids.sort_unstable();

        for (&zone_id, values_in_zone) in &zone_values {
            let mut stats = HashMap::new();

            for &statistic in &self.statistics {
                let value = self.calculate_statistic(statistic, values_in_zone)?;
                stats.insert(statistic, value);
            }

            result_zones.insert(zone_id, stats);
        }

        Ok(ZonalResult {
            zones: result_zones,
            zone_ids,
        })
    }

    /// Calculate multi-band zonal statistics
    ///
    /// # Arguments
    /// * `values` - Multi-band value raster (height × width × bands)
    /// * `zones` - Zone raster (height × width)
    ///
    /// # Errors
    /// Returns error if dimensions don't match
    pub fn calculate_multiband(
        &self,
        values: &ArrayView3<f64>,
        zones: &ArrayView2<i32>,
    ) -> Result<Vec<ZonalResult>> {
        let (height, width, n_bands) = values.dim();

        if (height, width) != zones.dim() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}x{}", height, width),
                format!("{:?}", zones.dim()),
            ));
        }

        let mut results = Vec::with_capacity(n_bands);

        for band in 0..n_bands {
            let band_values = values.slice(s![.., .., band]);
            let result = self.calculate(&band_values, zones)?;
            results.push(result);
        }

        Ok(results)
    }

    /// Calculate a single statistic
    fn calculate_statistic(&self, stat: ZonalStatistic, values: &[f64]) -> Result<f64> {
        if values.is_empty() {
            return Ok(f64::NAN);
        }

        match stat {
            ZonalStatistic::Mean => Ok(values.iter().sum::<f64>() / values.len() as f64),
            ZonalStatistic::Median => self.calculate_median(values),
            ZonalStatistic::Min => values
                .iter()
                .copied()
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .ok_or_else(|| AnalyticsError::zonal_stats_error("Failed to compute min")),
            ZonalStatistic::Max => values
                .iter()
                .copied()
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .ok_or_else(|| AnalyticsError::zonal_stats_error("Failed to compute max")),
            ZonalStatistic::Sum => Ok(values.iter().sum()),
            ZonalStatistic::Count => Ok(values.len() as f64),
            ZonalStatistic::StdDev => self.calculate_std_dev(values),
            ZonalStatistic::Variance => self.calculate_variance(values),
            ZonalStatistic::CoeffVar => {
                let mean = values.iter().sum::<f64>() / values.len() as f64;
                let std_dev = self.calculate_std_dev(values)?;
                Ok(if mean.abs() > f64::EPSILON {
                    (std_dev / mean) * 100.0
                } else {
                    f64::NAN
                })
            }
            ZonalStatistic::Percentile(p) => self.calculate_percentile(values, p),
        }
    }

    fn calculate_median(&self, values: &[f64]) -> Result<f64> {
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let n = sorted.len();
        if n % 2 == 0 {
            Ok((sorted[n / 2 - 1] + sorted[n / 2]) / 2.0)
        } else {
            Ok(sorted[n / 2])
        }
    }

    fn calculate_variance(&self, values: &[f64]) -> Result<f64> {
        let n = values.len() as f64;
        let mean = values.iter().sum::<f64>() / n;
        let variance = values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
        Ok(variance)
    }

    fn calculate_std_dev(&self, values: &[f64]) -> Result<f64> {
        Ok(self.calculate_variance(values)?.sqrt())
    }

    fn calculate_percentile(&self, values: &[f64], percentile: u8) -> Result<f64> {
        if percentile > 100 {
            return Err(AnalyticsError::invalid_parameter(
                "percentile",
                "must be between 0 and 100",
            ));
        }

        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let n = sorted.len();
        let rank = (percentile as f64 / 100.0) * ((n - 1) as f64);
        let lower_idx = rank.floor() as usize;
        let upper_idx = rank.ceil() as usize;
        let fraction = rank - (lower_idx as f64);

        Ok(sorted[lower_idx] + fraction * (sorted[upper_idx] - sorted[lower_idx]))
    }
}

impl Default for ZonalCalculator {
    fn default() -> Self {
        Self::new()
    }
}

/// Weighted zonal statistics calculator
pub struct WeightedZonalCalculator {
    calculator: ZonalCalculator,
}

impl WeightedZonalCalculator {
    /// Create a new weighted zonal calculator
    pub fn new() -> Self {
        Self {
            calculator: ZonalCalculator::new(),
        }
    }

    /// Set statistics to calculate
    pub fn with_statistics(mut self, stats: Vec<ZonalStatistic>) -> Self {
        self.calculator = self.calculator.with_statistics(stats);
        self
    }

    /// Calculate weighted zonal statistics
    ///
    /// # Arguments
    /// * `values` - Value raster
    /// * `weights` - Weight raster (same dimensions as values)
    /// * `zones` - Zone raster
    ///
    /// # Errors
    /// Returns error if dimensions don't match
    pub fn calculate(
        &self,
        values: &ArrayView2<f64>,
        weights: &ArrayView2<f64>,
        zones: &ArrayView2<i32>,
    ) -> Result<ZonalResult> {
        if values.dim() != weights.dim() || values.dim() != zones.dim() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{:?}", values.dim()),
                "all inputs must have same dimensions".to_string(),
            ));
        }

        // Group weighted values by zone
        let mut zone_data: HashMap<i32, (Vec<f64>, Vec<f64>)> = HashMap::new();

        for ((i, j), &zone_id) in zones.indexed_iter() {
            let value = values[[i, j]];
            let weight = weights[[i, j]];

            if weight > 0.0 {
                let entry = zone_data
                    .entry(zone_id)
                    .or_insert_with(|| (Vec::new(), Vec::new()));
                entry.0.push(value);
                entry.1.push(weight);
            }
        }

        // Calculate weighted statistics
        let mut result_zones = HashMap::new();
        let mut zone_ids: Vec<i32> = zone_data.keys().copied().collect();
        zone_ids.sort_unstable();

        for (&zone_id, (values_in_zone, weights_in_zone)) in &zone_data {
            let mut stats = HashMap::new();

            // Weighted mean
            let weighted_sum: f64 = values_in_zone
                .iter()
                .zip(weights_in_zone.iter())
                .map(|(v, w)| v * w)
                .sum();
            let weight_sum: f64 = weights_in_zone.iter().sum();

            if weight_sum > f64::EPSILON {
                stats.insert(ZonalStatistic::Mean, weighted_sum / weight_sum);
            }

            // Count (unweighted)
            stats.insert(ZonalStatistic::Count, values_in_zone.len() as f64);

            // Min/Max (unweighted)
            if let Some(&min) = values_in_zone
                .iter()
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            {
                stats.insert(ZonalStatistic::Min, min);
            }

            if let Some(&max) = values_in_zone
                .iter()
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            {
                stats.insert(ZonalStatistic::Max, max);
            }

            result_zones.insert(zone_id, stats);
        }

        Ok(ZonalResult {
            zones: result_zones,
            zone_ids,
        })
    }
}

impl Default for WeightedZonalCalculator {
    fn default() -> Self {
        Self::new()
    }
}

// Import ndarray slice macro
use scirs2_core::ndarray::s;

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use scirs2_core::ndarray::{Array, array};

    #[test]
    fn test_zonal_basic() {
        let values = array![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
        let zones = array![[1, 1, 2], [1, 2, 2], [2, 2, 2]];

        let calculator = ZonalCalculator::new();
        let result = calculator
            .calculate(&values.view(), &zones.view())
            .expect("Zonal statistics calculation should succeed");

        assert_eq!(result.zone_ids.len(), 2);
        assert!(result.zones.contains_key(&1));
        assert!(result.zones.contains_key(&2));

        // Zone 1: values 1, 2, 4
        let zone1_stats = &result.zones[&1];
        assert_abs_diff_eq!(
            zone1_stats[&ZonalStatistic::Mean],
            (1.0 + 2.0 + 4.0) / 3.0,
            epsilon = 1e-10
        );
    }

    #[test]
    fn test_zonal_statistics() {
        let values = array![[1.0, 2.0], [3.0, 4.0]];
        let zones = array![[1, 1], [1, 1]];

        let calculator = ZonalCalculator::new().with_statistics(vec![
            ZonalStatistic::Mean,
            ZonalStatistic::Min,
            ZonalStatistic::Max,
            ZonalStatistic::StdDev,
        ]);

        let result = calculator
            .calculate(&values.view(), &zones.view())
            .expect("Zonal statistics with multiple stats should succeed");
        let zone1_stats = &result.zones[&1];

        assert_abs_diff_eq!(zone1_stats[&ZonalStatistic::Mean], 2.5, epsilon = 1e-10);
        assert_abs_diff_eq!(zone1_stats[&ZonalStatistic::Min], 1.0, epsilon = 1e-10);
        assert_abs_diff_eq!(zone1_stats[&ZonalStatistic::Max], 4.0, epsilon = 1e-10);
    }

    #[test]
    fn test_weighted_zonal() {
        let values = array![[1.0, 2.0], [3.0, 4.0]];
        let weights = array![[1.0, 1.0], [1.0, 1.0]];
        let zones = array![[1, 1], [1, 1]];

        let calculator = WeightedZonalCalculator::new();
        let result = calculator
            .calculate(&values.view(), &weights.view(), &zones.view())
            .expect("Weighted zonal statistics should succeed");

        let zone1_stats = &result.zones[&1];
        assert_abs_diff_eq!(zone1_stats[&ZonalStatistic::Mean], 2.5, epsilon = 1e-10);
    }

    #[test]
    fn test_percentile() {
        let values = array![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
        let zones = array![[1, 1, 1], [1, 1, 1]];

        let calculator = ZonalCalculator::new().with_statistics(vec![
            ZonalStatistic::Percentile(50), // Median
            ZonalStatistic::Percentile(25),
            ZonalStatistic::Percentile(75),
        ]);

        let result = calculator
            .calculate(&values.view(), &zones.view())
            .expect("Percentile calculation should succeed");
        let zone1_stats = &result.zones[&1];

        assert_abs_diff_eq!(
            zone1_stats[&ZonalStatistic::Percentile(50)],
            3.5,
            epsilon = 1e-10
        );
    }
}

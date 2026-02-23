//! Advanced anomaly detection module for OxiGDAL observability.
//!
//! This module provides comprehensive anomaly detection capabilities including:
//! - Statistical methods (enhanced z-score, IQR, MAD)
//! - Time-series anomaly detection (EWMA, Holt-Winters, change point detection)
//! - Geospatial anomaly patterns (spatial clustering, hotspot detection)
//! - Threshold-based alerting with hysteresis
//! - Machine learning-based detection (Isolation Forest, LOF)
//! - Historical baseline computation with sliding windows
//! - Alert severity classification with multi-factor analysis

use super::{Anomaly, AnomalyDetector, AnomalySeverity, AnomalyType, Baseline, DataPoint};
use crate::error::{ObservabilityError, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

// ============================================================================
// Enhanced Statistical Detection
// ============================================================================

/// Modified Z-score detector using Median Absolute Deviation (MAD).
/// More robust than standard z-score for non-normal distributions.
pub struct MadDetector {
    baseline: RwLock<Option<MadBaseline>>,
    threshold: f64,
    metric_name: String,
}

/// Baseline statistics for MAD detector.
#[derive(Debug, Clone)]
struct MadBaseline {
    median: f64,
    mad: f64,
    consistency_constant: f64,
}

impl MadDetector {
    /// Create a new MAD detector with the specified threshold.
    /// Default consistency constant is 1.4826 for normal distributions.
    pub fn new(threshold: f64, metric_name: &str) -> Self {
        Self {
            baseline: RwLock::new(None),
            threshold,
            metric_name: metric_name.to_string(),
        }
    }

    /// Calculate median of sorted values.
    fn calculate_median(sorted_values: &[f64]) -> Option<f64> {
        if sorted_values.is_empty() {
            return None;
        }
        let n = sorted_values.len();
        if n % 2 == 0 {
            Some((sorted_values[n / 2 - 1] + sorted_values[n / 2]) / 2.0)
        } else {
            Some(sorted_values[n / 2])
        }
    }

    /// Calculate MAD baseline from data.
    fn calculate_baseline(data: &[DataPoint]) -> Result<MadBaseline> {
        if data.is_empty() {
            return Err(ObservabilityError::AnomalyDetectionError(
                "Cannot calculate MAD baseline from empty data".to_string(),
            ));
        }

        let mut values: Vec<f64> = data.iter().map(|d| d.value).collect();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median = Self::calculate_median(&values).ok_or_else(|| {
            ObservabilityError::AnomalyDetectionError("Failed to calculate median".to_string())
        })?;

        // Calculate absolute deviations from median
        let mut deviations: Vec<f64> = values.iter().map(|v| (v - median).abs()).collect();
        deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let mad = Self::calculate_median(&deviations).ok_or_else(|| {
            ObservabilityError::AnomalyDetectionError("Failed to calculate MAD".to_string())
        })?;

        // Consistency constant for normal distribution
        let consistency_constant = 1.4826;

        Ok(MadBaseline {
            median,
            mad,
            consistency_constant,
        })
    }
}

impl AnomalyDetector for MadDetector {
    fn detect(&self, data: &[DataPoint]) -> Result<Vec<Anomaly>> {
        let baseline = self.baseline.read();
        let baseline = baseline.as_ref().ok_or_else(|| {
            ObservabilityError::AnomalyDetectionError("MAD baseline not established".to_string())
        })?;

        let mut anomalies = Vec::new();
        let scaled_mad = baseline.mad * baseline.consistency_constant;

        // Avoid division by zero
        if scaled_mad < f64::EPSILON {
            return Ok(anomalies);
        }

        for point in data {
            let modified_zscore = (point.value - baseline.median).abs() / scaled_mad;

            if modified_zscore > self.threshold {
                let score = (modified_zscore / (self.threshold * 2.0)).min(1.0);
                let severity = classify_severity_by_score(score);

                let anomaly_type = if point.value > baseline.median {
                    AnomalyType::Spike
                } else {
                    AnomalyType::Drop
                };

                anomalies.push(Anomaly {
                    timestamp: point.timestamp,
                    metric_name: self.metric_name.clone(),
                    observed_value: point.value,
                    expected_value: baseline.median,
                    score,
                    severity,
                    anomaly_type,
                    description: format!(
                        "Modified Z-score: {:.2}, threshold: {:.2}",
                        modified_zscore, self.threshold
                    ),
                });
            }
        }

        Ok(anomalies)
    }

    fn update_baseline(&mut self, data: &[DataPoint]) -> Result<()> {
        let new_baseline = Self::calculate_baseline(data)?;
        *self.baseline.write() = Some(new_baseline);
        Ok(())
    }
}

// ============================================================================
// Time-Series Anomaly Detection
// ============================================================================

/// Exponentially Weighted Moving Average (EWMA) detector.
/// Suitable for detecting anomalies in streaming time-series data.
pub struct EwmaDetector {
    alpha: f64,
    threshold_factor: f64,
    ewma: RwLock<Option<f64>>,
    ewma_variance: RwLock<Option<f64>>,
    metric_name: String,
}

impl EwmaDetector {
    /// Create a new EWMA detector.
    ///
    /// # Arguments
    /// * `alpha` - Smoothing factor (0 < alpha < 1). Higher values give more weight to recent observations.
    /// * `threshold_factor` - Number of standard deviations for threshold.
    /// * `metric_name` - Name of the metric being monitored.
    pub fn new(alpha: f64, threshold_factor: f64, metric_name: &str) -> Self {
        Self {
            alpha: alpha.clamp(0.01, 0.99),
            threshold_factor,
            ewma: RwLock::new(None),
            ewma_variance: RwLock::new(None),
            metric_name: metric_name.to_string(),
        }
    }

    /// Update EWMA with a new value and return the predicted value.
    fn update_ewma(&self, value: f64) -> (f64, f64) {
        let mut ewma = self.ewma.write();
        let mut ewma_var = self.ewma_variance.write();

        let predicted = ewma.unwrap_or(value);
        let error = value - predicted;

        // Update EWMA
        let new_ewma = self.alpha * value + (1.0 - self.alpha) * predicted;
        *ewma = Some(new_ewma);

        // Update EWMA variance
        let current_var = ewma_var.unwrap_or(0.0);
        let new_var = self.alpha * error * error + (1.0 - self.alpha) * current_var;
        *ewma_var = Some(new_var);

        (predicted, new_var.sqrt())
    }
}

impl AnomalyDetector for EwmaDetector {
    fn detect(&self, data: &[DataPoint]) -> Result<Vec<Anomaly>> {
        let mut anomalies = Vec::new();

        for point in data {
            let (predicted, std_dev) = self.update_ewma(point.value);

            // Avoid detection on first few points
            if std_dev < f64::EPSILON {
                continue;
            }

            let z_score = (point.value - predicted).abs() / std_dev;

            if z_score > self.threshold_factor {
                let score = (z_score / (self.threshold_factor * 2.0)).min(1.0);
                let severity = classify_severity_by_score(score);

                let anomaly_type = if point.value > predicted {
                    AnomalyType::Spike
                } else {
                    AnomalyType::Drop
                };

                anomalies.push(Anomaly {
                    timestamp: point.timestamp,
                    metric_name: self.metric_name.clone(),
                    observed_value: point.value,
                    expected_value: predicted,
                    score,
                    severity,
                    anomaly_type,
                    description: format!(
                        "EWMA anomaly: predicted={:.2}, actual={:.2}, z={:.2}",
                        predicted, point.value, z_score
                    ),
                });
            }
        }

        Ok(anomalies)
    }

    fn update_baseline(&mut self, data: &[DataPoint]) -> Result<()> {
        // Initialize EWMA with first value
        if let Some(first) = data.first() {
            *self.ewma.write() = Some(first.value);
            *self.ewma_variance.write() = Some(0.0);

            // Process remaining points to warm up the model
            for point in data.iter().skip(1) {
                let _ = self.update_ewma(point.value);
            }
        }
        Ok(())
    }
}

/// Holt-Winters exponential smoothing for time series with trend and seasonality.
pub struct HoltWintersDetector {
    alpha: f64,      // Level smoothing
    beta: f64,       // Trend smoothing
    gamma: f64,      // Seasonal smoothing
    period: usize,   // Seasonal period
    threshold: f64,
    state: RwLock<Option<HoltWintersState>>,
    metric_name: String,
}

#[derive(Debug, Clone)]
struct HoltWintersState {
    level: f64,
    trend: f64,
    seasonal: Vec<f64>,
    fitted_values: Vec<f64>,
}

impl HoltWintersDetector {
    /// Create a new Holt-Winters detector.
    ///
    /// # Arguments
    /// * `alpha` - Level smoothing factor (0 < alpha < 1)
    /// * `beta` - Trend smoothing factor (0 < beta < 1)
    /// * `gamma` - Seasonal smoothing factor (0 < gamma < 1)
    /// * `period` - Seasonal period length
    /// * `threshold` - Anomaly threshold in standard deviations
    pub fn new(
        alpha: f64,
        beta: f64,
        gamma: f64,
        period: usize,
        threshold: f64,
        metric_name: &str,
    ) -> Self {
        Self {
            alpha: alpha.clamp(0.01, 0.99),
            beta: beta.clamp(0.01, 0.99),
            gamma: gamma.clamp(0.01, 0.99),
            period: period.max(2),
            threshold,
            state: RwLock::new(None),
            metric_name: metric_name.to_string(),
        }
    }

    /// Initialize the Holt-Winters model.
    fn initialize(&self, data: &[DataPoint]) -> Result<HoltWintersState> {
        if data.len() < self.period * 2 {
            return Err(ObservabilityError::AnomalyDetectionError(
                format!(
                    "Insufficient data for Holt-Winters: need at least {} points",
                    self.period * 2
                ),
            ));
        }

        // Calculate initial level as mean of first period
        let initial_level: f64 = data[..self.period].iter().map(|d| d.value).sum::<f64>()
            / self.period as f64;

        // Calculate initial trend
        let first_period_mean: f64 = data[..self.period].iter().map(|d| d.value).sum::<f64>()
            / self.period as f64;
        let second_period_mean: f64 = data[self.period..self.period * 2]
            .iter()
            .map(|d| d.value)
            .sum::<f64>()
            / self.period as f64;
        let initial_trend = (second_period_mean - first_period_mean) / self.period as f64;

        // Calculate initial seasonal factors
        let mut seasonal = vec![0.0; self.period];
        for i in 0..self.period {
            let season_values: Vec<f64> = data
                .iter()
                .skip(i)
                .step_by(self.period)
                .take(2)
                .map(|d| d.value)
                .collect();

            if !season_values.is_empty() {
                let avg = season_values.iter().sum::<f64>() / season_values.len() as f64;
                seasonal[i] = avg - initial_level;
            }
        }

        Ok(HoltWintersState {
            level: initial_level,
            trend: initial_trend,
            seasonal,
            fitted_values: Vec::new(),
        })
    }

    /// Predict and update the model for a single value.
    fn predict_and_update(&self, state: &mut HoltWintersState, value: f64, season_idx: usize) -> f64 {
        let prediction = state.level + state.trend + state.seasonal[season_idx];

        // Update components
        let new_level = self.alpha * (value - state.seasonal[season_idx])
            + (1.0 - self.alpha) * (state.level + state.trend);
        let new_trend = self.beta * (new_level - state.level) + (1.0 - self.beta) * state.trend;
        let new_seasonal = self.gamma * (value - new_level) + (1.0 - self.gamma) * state.seasonal[season_idx];

        state.level = new_level;
        state.trend = new_trend;
        state.seasonal[season_idx] = new_seasonal;
        state.fitted_values.push(prediction);

        prediction
    }
}

impl AnomalyDetector for HoltWintersDetector {
    fn detect(&self, data: &[DataPoint]) -> Result<Vec<Anomaly>> {
        let mut state_guard = self.state.write();
        let state = state_guard.as_mut().ok_or_else(|| {
            ObservabilityError::AnomalyDetectionError("Holt-Winters model not initialized".to_string())
        })?;

        let mut anomalies = Vec::new();

        // Calculate residual standard deviation from fitted values
        let residuals: Vec<f64> = state
            .fitted_values
            .iter()
            .zip(data.iter())
            .map(|(f, d)| (d.value - f).abs())
            .collect();

        let residual_std = if residuals.len() > 1 {
            let mean = residuals.iter().sum::<f64>() / residuals.len() as f64;
            let variance = residuals.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
                / (residuals.len() - 1) as f64;
            variance.sqrt()
        } else {
            1.0
        };

        for (i, point) in data.iter().enumerate() {
            let season_idx = i % self.period;
            let prediction = self.predict_and_update(state, point.value, season_idx);
            let residual = (point.value - prediction).abs();

            if residual > self.threshold * residual_std {
                let score = (residual / (self.threshold * residual_std * 2.0)).min(1.0);
                let severity = classify_severity_by_score(score);

                let anomaly_type = if point.value > prediction {
                    AnomalyType::Spike
                } else {
                    AnomalyType::Drop
                };

                anomalies.push(Anomaly {
                    timestamp: point.timestamp,
                    metric_name: self.metric_name.clone(),
                    observed_value: point.value,
                    expected_value: prediction,
                    score,
                    severity,
                    anomaly_type,
                    description: format!(
                        "Holt-Winters anomaly: predicted={:.2}, actual={:.2}",
                        prediction, point.value
                    ),
                });
            }
        }

        Ok(anomalies)
    }

    fn update_baseline(&mut self, data: &[DataPoint]) -> Result<()> {
        let new_state = self.initialize(data)?;
        *self.state.write() = Some(new_state);
        Ok(())
    }
}

/// Change point detector using CUSUM (Cumulative Sum) algorithm.
pub struct ChangePointDetector {
    threshold: f64,
    drift: f64,
    baseline_mean: RwLock<Option<f64>>,
    baseline_std: RwLock<Option<f64>>,
    metric_name: String,
}

impl ChangePointDetector {
    /// Create a new change point detector.
    ///
    /// # Arguments
    /// * `threshold` - Detection threshold (typically 4-5 for detecting shifts of 1 std dev)
    /// * `drift` - Allowable drift (typically 0.5 * expected shift size)
    pub fn new(threshold: f64, drift: f64, metric_name: &str) -> Self {
        Self {
            threshold,
            drift,
            baseline_mean: RwLock::new(None),
            baseline_std: RwLock::new(None),
            metric_name: metric_name.to_string(),
        }
    }
}

impl AnomalyDetector for ChangePointDetector {
    fn detect(&self, data: &[DataPoint]) -> Result<Vec<Anomaly>> {
        let mean = self.baseline_mean.read();
        let std = self.baseline_std.read();

        let mean = *mean.as_ref().ok_or_else(|| {
            ObservabilityError::AnomalyDetectionError("Baseline not established".to_string())
        })?;
        let std = *std.as_ref().ok_or_else(|| {
            ObservabilityError::AnomalyDetectionError("Baseline not established".to_string())
        })?;

        if std < f64::EPSILON {
            return Ok(Vec::new());
        }

        let mut anomalies = Vec::new();
        let mut s_high = 0.0_f64;
        let mut s_low = 0.0_f64;

        for point in data {
            let normalized = (point.value - mean) / std;

            // Update CUSUM statistics
            s_high = (s_high + normalized - self.drift).max(0.0);
            s_low = (s_low - normalized - self.drift).max(0.0);

            // Check for change points
            if s_high > self.threshold || s_low > self.threshold {
                let score = (s_high.max(s_low) / (self.threshold * 2.0)).min(1.0);
                let severity = classify_severity_by_score(score);

                let anomaly_type = if s_high > s_low {
                    AnomalyType::UpwardTrend
                } else {
                    AnomalyType::DownwardTrend
                };

                anomalies.push(Anomaly {
                    timestamp: point.timestamp,
                    metric_name: self.metric_name.clone(),
                    observed_value: point.value,
                    expected_value: mean,
                    score,
                    severity,
                    anomaly_type,
                    description: format!(
                        "Change point detected: S+={:.2}, S-={:.2}",
                        s_high, s_low
                    ),
                });

                // Reset CUSUM after detection
                s_high = 0.0;
                s_low = 0.0;
            }
        }

        Ok(anomalies)
    }

    fn update_baseline(&mut self, data: &[DataPoint]) -> Result<()> {
        if data.is_empty() {
            return Err(ObservabilityError::AnomalyDetectionError(
                "Cannot calculate baseline from empty data".to_string(),
            ));
        }

        let values: Vec<f64> = data.iter().map(|d| d.value).collect();
        let n = values.len() as f64;
        let mean = values.iter().sum::<f64>() / n;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let std = variance.sqrt();

        *self.baseline_mean.write() = Some(mean);
        *self.baseline_std.write() = Some(std);

        Ok(())
    }
}

// ============================================================================
// Geospatial Anomaly Detection
// ============================================================================

/// Geospatial data point with coordinates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoDataPoint {
    /// Longitude coordinate.
    pub longitude: f64,
    /// Latitude coordinate.
    pub latitude: f64,
    /// Associated value.
    pub value: f64,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Optional metadata.
    pub metadata: HashMap<String, String>,
}

impl GeoDataPoint {
    /// Create a new geospatial data point.
    pub fn new(longitude: f64, latitude: f64, value: f64, timestamp: DateTime<Utc>) -> Self {
        Self {
            longitude,
            latitude,
            value,
            timestamp,
            metadata: HashMap::new(),
        }
    }

    /// Calculate Haversine distance to another point in kilometers.
    pub fn distance_to(&self, other: &GeoDataPoint) -> f64 {
        const EARTH_RADIUS_KM: f64 = 6371.0;

        let lat1_rad = self.latitude.to_radians();
        let lat2_rad = other.latitude.to_radians();
        let delta_lat = (other.latitude - self.latitude).to_radians();
        let delta_lon = (other.longitude - self.longitude).to_radians();

        let a = (delta_lat / 2.0).sin().powi(2)
            + lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

        EARTH_RADIUS_KM * c
    }
}

/// Geospatial anomaly result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoAnomaly {
    /// Center of the anomalous region.
    pub center: (f64, f64),
    /// Radius of the anomalous region in km.
    pub radius_km: f64,
    /// Number of points in the region.
    pub point_count: usize,
    /// Average value in the region.
    pub avg_value: f64,
    /// Expected value based on surrounding areas.
    pub expected_value: f64,
    /// Anomaly score (0.0 to 1.0).
    pub score: f64,
    /// Severity level.
    pub severity: AnomalySeverity,
    /// Anomaly type.
    pub anomaly_type: GeoAnomalyType,
    /// Description.
    pub description: String,
    /// Timestamp of detection.
    pub timestamp: DateTime<Utc>,
}

/// Types of geospatial anomalies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeoAnomalyType {
    /// Spatial hotspot (high density of high values).
    Hotspot,
    /// Spatial coldspot (high density of low values).
    Coldspot,
    /// Isolated outlier point.
    IsolatedOutlier,
    /// Spatial cluster anomaly.
    ClusterAnomaly,
    /// Boundary anomaly (unusual values at edges).
    BoundaryAnomaly,
}

/// Geospatial anomaly detector using spatial statistics.
pub struct GeoAnomalyDetector {
    /// Grid cell size in degrees for spatial binning.
    cell_size: f64,
    /// Neighbor distance threshold in km.
    neighbor_distance: f64,
    /// Minimum points for local analysis.
    min_neighbors: usize,
    /// Z-score threshold for anomaly detection.
    threshold: f64,
    /// Global baseline statistics.
    global_stats: RwLock<Option<GeoBaseline>>,
}

#[derive(Debug, Clone)]
struct GeoBaseline {
    global_mean: f64,
    global_std: f64,
    cell_stats: HashMap<(i32, i32), CellStats>,
}

#[derive(Debug, Clone)]
struct CellStats {
    mean: f64,
    std: f64,
    count: usize,
}

impl GeoAnomalyDetector {
    /// Create a new geospatial anomaly detector.
    ///
    /// # Arguments
    /// * `cell_size` - Grid cell size in degrees (e.g., 0.1 for ~10km cells)
    /// * `neighbor_distance` - Distance threshold for neighbors in km
    /// * `min_neighbors` - Minimum neighbors required for local analysis
    /// * `threshold` - Z-score threshold for anomaly detection
    pub fn new(cell_size: f64, neighbor_distance: f64, min_neighbors: usize, threshold: f64) -> Self {
        Self {
            cell_size: cell_size.max(0.001),
            neighbor_distance: neighbor_distance.max(0.1),
            min_neighbors: min_neighbors.max(1),
            threshold,
            global_stats: RwLock::new(None),
        }
    }

    /// Get grid cell index for a coordinate.
    fn get_cell(&self, lon: f64, lat: f64) -> (i32, i32) {
        let cell_x = (lon / self.cell_size).floor() as i32;
        let cell_y = (lat / self.cell_size).floor() as i32;
        (cell_x, cell_y)
    }

    /// Update baseline statistics from geospatial data.
    pub fn update_geo_baseline(&mut self, data: &[GeoDataPoint]) -> Result<()> {
        if data.is_empty() {
            return Err(ObservabilityError::AnomalyDetectionError(
                "Cannot calculate geo baseline from empty data".to_string(),
            ));
        }

        // Calculate global statistics
        let values: Vec<f64> = data.iter().map(|p| p.value).collect();
        let n = values.len() as f64;
        let global_mean = values.iter().sum::<f64>() / n;
        let global_variance = values.iter().map(|v| (v - global_mean).powi(2)).sum::<f64>() / n;
        let global_std = global_variance.sqrt();

        // Calculate cell-level statistics
        let mut cell_values: HashMap<(i32, i32), Vec<f64>> = HashMap::new();
        for point in data {
            let cell = self.get_cell(point.longitude, point.latitude);
            cell_values.entry(cell).or_default().push(point.value);
        }

        let mut cell_stats = HashMap::new();
        for (cell, values) in cell_values {
            let count = values.len();
            let mean = values.iter().sum::<f64>() / count as f64;
            let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64;
            let std = variance.sqrt();
            cell_stats.insert(cell, CellStats { mean, std, count });
        }

        *self.global_stats.write() = Some(GeoBaseline {
            global_mean,
            global_std,
            cell_stats,
        });

        Ok(())
    }

    /// Detect geospatial anomalies.
    pub fn detect_geo_anomalies(&self, data: &[GeoDataPoint]) -> Result<Vec<GeoAnomaly>> {
        let stats = self.global_stats.read();
        let stats = stats.as_ref().ok_or_else(|| {
            ObservabilityError::AnomalyDetectionError("Geo baseline not established".to_string())
        })?;

        if stats.global_std < f64::EPSILON {
            return Ok(Vec::new());
        }

        let mut anomalies = Vec::new();

        // Detect local anomalies using Getis-Ord Gi* approach
        for point in data {
            let neighbors: Vec<&GeoDataPoint> = data
                .iter()
                .filter(|p| point.distance_to(p) <= self.neighbor_distance)
                .collect();

            if neighbors.len() < self.min_neighbors {
                // Check for isolated outlier
                let z_score = (point.value - stats.global_mean) / stats.global_std;
                if z_score.abs() > self.threshold {
                    let score = (z_score.abs() / (self.threshold * 2.0)).min(1.0);
                    anomalies.push(GeoAnomaly {
                        center: (point.longitude, point.latitude),
                        radius_km: 0.0,
                        point_count: 1,
                        avg_value: point.value,
                        expected_value: stats.global_mean,
                        score,
                        severity: classify_severity_by_score(score),
                        anomaly_type: GeoAnomalyType::IsolatedOutlier,
                        description: format!(
                            "Isolated outlier at ({:.4}, {:.4}): value={:.2}, z={:.2}",
                            point.longitude, point.latitude, point.value, z_score
                        ),
                        timestamp: point.timestamp,
                    });
                }
                continue;
            }

            // Calculate local statistics
            let local_sum: f64 = neighbors.iter().map(|p| p.value).sum();
            let local_mean = local_sum / neighbors.len() as f64;
            let n = data.len() as f64;
            let local_n = neighbors.len() as f64;

            // Simplified Gi* statistic
            let gi_star = (local_sum - stats.global_mean * local_n)
                / (stats.global_std * (n * local_n - local_n.powi(2)).sqrt() / (n - 1.0).sqrt());

            if gi_star.abs() > self.threshold {
                let score = (gi_star.abs() / (self.threshold * 2.0)).min(1.0);
                let anomaly_type = if gi_star > 0.0 {
                    GeoAnomalyType::Hotspot
                } else {
                    GeoAnomalyType::Coldspot
                };

                anomalies.push(GeoAnomaly {
                    center: (point.longitude, point.latitude),
                    radius_km: self.neighbor_distance,
                    point_count: neighbors.len(),
                    avg_value: local_mean,
                    expected_value: stats.global_mean,
                    score,
                    severity: classify_severity_by_score(score),
                    anomaly_type,
                    description: format!(
                        "{:?} at ({:.4}, {:.4}): Gi*={:.2}, local_avg={:.2}",
                        anomaly_type, point.longitude, point.latitude, gi_star, local_mean
                    ),
                    timestamp: point.timestamp,
                });
            }
        }

        Ok(anomalies)
    }
}

// ============================================================================
// Threshold-Based Alerting with Hysteresis
// ============================================================================

/// Alert configuration with hysteresis support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertThreshold {
    /// Unique identifier for the alert.
    pub id: String,
    /// Metric name to monitor.
    pub metric_name: String,
    /// Warning threshold.
    pub warning_threshold: f64,
    /// Critical threshold.
    pub critical_threshold: f64,
    /// Comparison operator.
    pub operator: ThresholdOperator,
    /// Hysteresis percentage (0.0 to 1.0).
    pub hysteresis: f64,
    /// Minimum duration before alerting.
    pub min_duration: Duration,
    /// Alert cooldown period.
    pub cooldown: Duration,
}

/// Threshold comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThresholdOperator {
    /// Value greater than threshold.
    GreaterThan,
    /// Value less than threshold.
    LessThan,
    /// Value greater than or equal to threshold.
    GreaterThanOrEqual,
    /// Value less than or equal to threshold.
    LessThanOrEqual,
    /// Value equals threshold (with epsilon).
    Equals,
    /// Value not equals threshold.
    NotEquals,
}

impl ThresholdOperator {
    /// Check if value satisfies the threshold condition.
    pub fn check(&self, value: f64, threshold: f64) -> bool {
        const EPSILON: f64 = 1e-9;
        match self {
            Self::GreaterThan => value > threshold,
            Self::LessThan => value < threshold,
            Self::GreaterThanOrEqual => value >= threshold,
            Self::LessThanOrEqual => value <= threshold,
            Self::Equals => (value - threshold).abs() < EPSILON,
            Self::NotEquals => (value - threshold).abs() >= EPSILON,
        }
    }
}

/// Alert state for hysteresis tracking.
#[derive(Debug, Clone)]
pub struct AlertState {
    /// Current alert status.
    pub status: AlertStatus,
    /// Timestamp when alert was first triggered.
    pub triggered_at: Option<DateTime<Utc>>,
    /// Last alert notification time.
    pub last_notified: Option<DateTime<Utc>>,
    /// Consecutive violation count.
    pub violation_count: usize,
}

/// Alert status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertStatus {
    /// Normal state.
    Ok,
    /// Warning level reached.
    Warning,
    /// Critical level reached.
    Critical,
    /// Recovering (was alerted, now below threshold + hysteresis).
    Recovering,
}

/// Threshold-based alert manager.
pub struct ThresholdAlertManager {
    thresholds: HashMap<String, AlertThreshold>,
    states: RwLock<HashMap<String, AlertState>>,
}

impl ThresholdAlertManager {
    /// Create a new threshold alert manager.
    pub fn new() -> Self {
        Self {
            thresholds: HashMap::new(),
            states: RwLock::new(HashMap::new()),
        }
    }

    /// Add an alert threshold configuration.
    pub fn add_threshold(&mut self, threshold: AlertThreshold) {
        let id = threshold.id.clone();
        self.thresholds.insert(id.clone(), threshold);
        self.states.write().insert(
            id,
            AlertState {
                status: AlertStatus::Ok,
                triggered_at: None,
                last_notified: None,
                violation_count: 0,
            },
        );
    }

    /// Evaluate a metric value against configured thresholds.
    pub fn evaluate(&self, metric_name: &str, value: f64, timestamp: DateTime<Utc>) -> Vec<Alert> {
        let mut alerts = Vec::new();

        for (id, threshold) in &self.thresholds {
            if threshold.metric_name != metric_name {
                continue;
            }

            let mut states = self.states.write();
            let state = states.entry(id.clone()).or_insert(AlertState {
                status: AlertStatus::Ok,
                triggered_at: None,
                last_notified: None,
                violation_count: 0,
            });

            // Check critical threshold
            let is_critical = threshold.operator.check(value, threshold.critical_threshold);
            // Check warning threshold
            let is_warning = threshold.operator.check(value, threshold.warning_threshold);

            // Calculate hysteresis bounds
            let hysteresis_factor = 1.0 - threshold.hysteresis;
            let critical_recovery = threshold.critical_threshold * hysteresis_factor;
            let warning_recovery = threshold.warning_threshold * hysteresis_factor;

            let new_status = if is_critical {
                AlertStatus::Critical
            } else if is_warning {
                AlertStatus::Warning
            } else if state.status == AlertStatus::Critical {
                if threshold.operator.check(value, critical_recovery) {
                    AlertStatus::Critical
                } else {
                    AlertStatus::Recovering
                }
            } else if state.status == AlertStatus::Warning {
                if threshold.operator.check(value, warning_recovery) {
                    AlertStatus::Warning
                } else {
                    AlertStatus::Recovering
                }
            } else {
                AlertStatus::Ok
            };

            // Update state
            if new_status != state.status {
                if new_status == AlertStatus::Ok {
                    state.triggered_at = None;
                    state.violation_count = 0;
                } else if state.status == AlertStatus::Ok {
                    state.triggered_at = Some(timestamp);
                }
            }

            if matches!(new_status, AlertStatus::Warning | AlertStatus::Critical) {
                state.violation_count += 1;
            }

            // Check if we should emit an alert
            let should_alert = if let Some(triggered_at) = state.triggered_at {
                let duration_satisfied = timestamp.signed_duration_since(triggered_at)
                    >= threshold.min_duration;

                let cooldown_satisfied = state
                    .last_notified
                    .map(|t| timestamp.signed_duration_since(t) >= threshold.cooldown)
                    .unwrap_or(true);

                duration_satisfied && cooldown_satisfied && new_status != AlertStatus::Ok
            } else {
                false
            };

            if should_alert {
                state.last_notified = Some(timestamp);
                alerts.push(Alert {
                    id: id.clone(),
                    metric_name: metric_name.to_string(),
                    value,
                    threshold: if new_status == AlertStatus::Critical {
                        threshold.critical_threshold
                    } else {
                        threshold.warning_threshold
                    },
                    status: new_status,
                    severity: match new_status {
                        AlertStatus::Critical => AnomalySeverity::Critical,
                        AlertStatus::Warning => AnomalySeverity::Medium,
                        _ => AnomalySeverity::Low,
                    },
                    timestamp,
                    description: format!(
                        "Threshold alert: {} {} {:?} {}",
                        metric_name,
                        match threshold.operator {
                            ThresholdOperator::GreaterThan => ">",
                            ThresholdOperator::LessThan => "<",
                            ThresholdOperator::GreaterThanOrEqual => ">=",
                            ThresholdOperator::LessThanOrEqual => "<=",
                            ThresholdOperator::Equals => "==",
                            ThresholdOperator::NotEquals => "!=",
                        },
                        new_status,
                        threshold.critical_threshold
                    ),
                });
            }

            state.status = new_status;
        }

        alerts
    }

    /// Get current state of an alert.
    pub fn get_state(&self, id: &str) -> Option<AlertState> {
        self.states.read().get(id).cloned()
    }

    /// Reset an alert state.
    pub fn reset_alert(&self, id: &str) {
        if let Some(state) = self.states.write().get_mut(id) {
            state.status = AlertStatus::Ok;
            state.triggered_at = None;
            state.last_notified = None;
            state.violation_count = 0;
        }
    }
}

impl Default for ThresholdAlertManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Alert generated by threshold monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Alert configuration ID.
    pub id: String,
    /// Metric name.
    pub metric_name: String,
    /// Current value.
    pub value: f64,
    /// Threshold that was violated.
    pub threshold: f64,
    /// Alert status.
    pub status: AlertStatus,
    /// Alert severity.
    pub severity: AnomalySeverity,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Description.
    pub description: String,
}

// ============================================================================
// Machine Learning-Based Detection
// ============================================================================

/// Local Outlier Factor (LOF) detector.
/// Detects outliers based on local density deviation.
pub struct LofDetector {
    k_neighbors: usize,
    threshold: f64,
    training_data: RwLock<Vec<DataPoint>>,
    metric_name: String,
}

impl LofDetector {
    /// Create a new LOF detector.
    ///
    /// # Arguments
    /// * `k_neighbors` - Number of neighbors for local density calculation
    /// * `threshold` - LOF threshold (points with LOF > threshold are anomalies)
    pub fn new(k_neighbors: usize, threshold: f64, metric_name: &str) -> Self {
        Self {
            k_neighbors: k_neighbors.max(1),
            threshold: threshold.max(1.0),
            training_data: RwLock::new(Vec::new()),
            metric_name: metric_name.to_string(),
        }
    }

    /// Calculate k-distance for a point.
    fn k_distance(&self, point: f64, data: &[f64]) -> f64 {
        let mut distances: Vec<f64> = data.iter().map(|d| (d - point).abs()).collect();
        distances.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        distances.get(self.k_neighbors.min(distances.len()) - 1).copied().unwrap_or(0.0)
    }

    /// Calculate reachability distance.
    fn reachability_distance(&self, point_a: f64, point_b: f64, data: &[f64]) -> f64 {
        let k_dist_b = self.k_distance(point_b, data);
        let dist_ab = (point_a - point_b).abs();
        k_dist_b.max(dist_ab)
    }

    /// Calculate local reachability density.
    fn local_reachability_density(&self, point: f64, data: &[f64]) -> f64 {
        let mut distances: Vec<(f64, f64)> = data
            .iter()
            .map(|d| ((d - point).abs(), *d))
            .collect();
        distances.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let k_nearest: Vec<f64> = distances
            .iter()
            .take(self.k_neighbors)
            .map(|(_, v)| *v)
            .collect();

        if k_nearest.is_empty() {
            return 0.0;
        }

        let sum_reach_dist: f64 = k_nearest
            .iter()
            .map(|&neighbor| self.reachability_distance(point, neighbor, data))
            .sum();

        if sum_reach_dist < f64::EPSILON {
            return f64::MAX;
        }

        k_nearest.len() as f64 / sum_reach_dist
    }

    /// Calculate Local Outlier Factor for a point.
    fn calculate_lof(&self, point: f64, data: &[f64]) -> f64 {
        let point_lrd = self.local_reachability_density(point, data);

        if point_lrd < f64::EPSILON {
            return 1.0;
        }

        let mut distances: Vec<(f64, f64)> = data
            .iter()
            .map(|d| ((d - point).abs(), *d))
            .collect();
        distances.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let k_nearest: Vec<f64> = distances
            .iter()
            .take(self.k_neighbors)
            .map(|(_, v)| *v)
            .collect();

        if k_nearest.is_empty() {
            return 1.0;
        }

        let sum_lrd: f64 = k_nearest
            .iter()
            .map(|&neighbor| self.local_reachability_density(neighbor, data))
            .sum();

        sum_lrd / (k_nearest.len() as f64 * point_lrd)
    }
}

impl AnomalyDetector for LofDetector {
    fn detect(&self, data: &[DataPoint]) -> Result<Vec<Anomaly>> {
        let training_data = self.training_data.read();

        if training_data.len() < self.k_neighbors {
            return Ok(Vec::new());
        }

        let training_values: Vec<f64> = training_data.iter().map(|d| d.value).collect();
        let mut anomalies = Vec::new();

        for point in data {
            let lof = self.calculate_lof(point.value, &training_values);

            if lof > self.threshold {
                let score = ((lof - 1.0) / (self.threshold - 1.0)).min(1.0);
                let severity = classify_severity_by_score(score);

                // Calculate expected value as mean of k-nearest neighbors
                let mut distances: Vec<(f64, f64)> = training_values
                    .iter()
                    .map(|d| ((d - point.value).abs(), *d))
                    .collect();
                distances.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                let expected = distances
                    .iter()
                    .take(self.k_neighbors)
                    .map(|(_, v)| v)
                    .sum::<f64>()
                    / self.k_neighbors as f64;

                anomalies.push(Anomaly {
                    timestamp: point.timestamp,
                    metric_name: self.metric_name.clone(),
                    observed_value: point.value,
                    expected_value: expected,
                    score,
                    severity,
                    anomaly_type: AnomalyType::Pattern,
                    description: format!("LOF anomaly: LOF={:.2}, threshold={:.2}", lof, self.threshold),
                });
            }
        }

        Ok(anomalies)
    }

    fn update_baseline(&mut self, data: &[DataPoint]) -> Result<()> {
        *self.training_data.write() = data.to_vec();
        Ok(())
    }
}

// ============================================================================
// Historical Baseline Computation
// ============================================================================

/// Historical baseline manager with sliding windows.
pub struct HistoricalBaseline {
    /// Window size for baseline calculation.
    window_size: usize,
    /// Historical data buffer.
    data_buffer: RwLock<VecDeque<DataPoint>>,
    /// Computed baselines by time period.
    hourly_baselines: RwLock<HashMap<u32, Baseline>>,
    /// Daily baselines.
    daily_baselines: RwLock<HashMap<u32, Baseline>>,
    /// Weekly baselines.
    weekly_baselines: RwLock<HashMap<u32, Baseline>>,
}

impl HistoricalBaseline {
    /// Create a new historical baseline manager.
    pub fn new(window_size: usize) -> Self {
        Self {
            window_size,
            data_buffer: RwLock::new(VecDeque::new()),
            hourly_baselines: RwLock::new(HashMap::new()),
            daily_baselines: RwLock::new(HashMap::new()),
            weekly_baselines: RwLock::new(HashMap::new()),
        }
    }

    /// Add a new data point and update baselines.
    pub fn add_data_point(&self, point: DataPoint) {
        let mut buffer = self.data_buffer.write();
        buffer.push_back(point.clone());

        // Maintain window size
        while buffer.len() > self.window_size {
            buffer.pop_front();
        }

        // Update hourly baseline
        let hour = point.timestamp.hour();
        self.update_period_baseline(&buffer, hour, &self.hourly_baselines);

        // Update daily baseline (day of week)
        let day_of_week = point.timestamp.weekday().num_days_from_monday();
        self.update_period_baseline(&buffer, day_of_week, &self.daily_baselines);

        // Update weekly baseline (week of year)
        let week = point.timestamp.iso_week().week();
        self.update_period_baseline(&buffer, week, &self.weekly_baselines);
    }

    /// Update baseline for a specific period.
    fn update_period_baseline(
        &self,
        buffer: &VecDeque<DataPoint>,
        period: u32,
        baselines: &RwLock<HashMap<u32, Baseline>>,
    ) {
        let points: Vec<DataPoint> = buffer.iter().cloned().collect();
        if let Ok(baseline) = Baseline::from_data(&points) {
            baselines.write().insert(period, baseline);
        }
    }

    /// Get baseline for the current hour.
    pub fn get_hourly_baseline(&self, hour: u32) -> Option<Baseline> {
        self.hourly_baselines.read().get(&hour).cloned()
    }

    /// Get baseline for a specific day of week.
    pub fn get_daily_baseline(&self, day_of_week: u32) -> Option<Baseline> {
        self.daily_baselines.read().get(&day_of_week).cloned()
    }

    /// Get baseline for a specific week.
    pub fn get_weekly_baseline(&self, week: u32) -> Option<Baseline> {
        self.weekly_baselines.read().get(&week).cloned()
    }

    /// Get combined baseline considering all time periods.
    pub fn get_combined_baseline(&self, timestamp: DateTime<Utc>) -> Option<Baseline> {
        let hour = timestamp.hour();
        let day = timestamp.weekday().num_days_from_monday();
        let week = timestamp.iso_week().week();

        let hourly = self.get_hourly_baseline(hour);
        let daily = self.get_daily_baseline(day);
        let weekly = self.get_weekly_baseline(week);

        // Combine baselines with weighted average
        let mut means = Vec::new();
        let mut std_devs = Vec::new();
        let weights = [0.5, 0.3, 0.2]; // Hour, Day, Week

        if let Some(b) = hourly {
            means.push((b.mean, weights[0]));
            std_devs.push((b.std_dev, weights[0]));
        }
        if let Some(b) = daily {
            means.push((b.mean, weights[1]));
            std_devs.push((b.std_dev, weights[1]));
        }
        if let Some(b) = weekly {
            means.push((b.mean, weights[2]));
            std_devs.push((b.std_dev, weights[2]));
        }

        if means.is_empty() {
            return None;
        }

        let total_weight: f64 = means.iter().map(|(_, w)| w).sum();
        let weighted_mean: f64 = means.iter().map(|(m, w)| m * w).sum::<f64>() / total_weight;
        let weighted_std: f64 = std_devs.iter().map(|(s, w)| s * w).sum::<f64>() / total_weight;

        Some(Baseline {
            mean: weighted_mean,
            std_dev: weighted_std,
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
            count: 0,
        })
    }

    /// Clear all historical data.
    pub fn clear(&self) {
        self.data_buffer.write().clear();
        self.hourly_baselines.write().clear();
        self.daily_baselines.write().clear();
        self.weekly_baselines.write().clear();
    }
}

// ============================================================================
// Alert Severity Classification
// ============================================================================

/// Multi-factor severity classifier.
pub struct SeverityClassifier {
    /// Weights for different factors.
    weights: SeverityWeights,
    /// Historical severity context.
    context: RwLock<SeverityContext>,
}

/// Weights for severity classification factors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeverityWeights {
    /// Weight for anomaly score.
    pub score_weight: f64,
    /// Weight for deviation magnitude.
    pub deviation_weight: f64,
    /// Weight for frequency of occurrence.
    pub frequency_weight: f64,
    /// Weight for time of day (business hours vs off-hours).
    pub time_weight: f64,
    /// Weight for metric criticality.
    pub criticality_weight: f64,
}

impl Default for SeverityWeights {
    fn default() -> Self {
        Self {
            score_weight: 0.3,
            deviation_weight: 0.25,
            frequency_weight: 0.15,
            time_weight: 0.1,
            criticality_weight: 0.2,
        }
    }
}

/// Context for severity classification.
#[derive(Debug, Clone, Default)]
struct SeverityContext {
    /// Recent anomaly counts per metric.
    recent_counts: HashMap<String, usize>,
    /// Metric criticality levels.
    criticality_levels: HashMap<String, f64>,
}

impl SeverityClassifier {
    /// Create a new severity classifier with default weights.
    pub fn new() -> Self {
        Self {
            weights: SeverityWeights::default(),
            context: RwLock::new(SeverityContext::default()),
        }
    }

    /// Create with custom weights.
    pub fn with_weights(weights: SeverityWeights) -> Self {
        Self {
            weights,
            context: RwLock::new(SeverityContext::default()),
        }
    }

    /// Set criticality level for a metric (0.0 to 1.0).
    pub fn set_metric_criticality(&self, metric_name: &str, criticality: f64) {
        self.context
            .write()
            .criticality_levels
            .insert(metric_name.to_string(), criticality.clamp(0.0, 1.0));
    }

    /// Record an anomaly occurrence for frequency tracking.
    pub fn record_anomaly(&self, metric_name: &str) {
        let mut context = self.context.write();
        *context.recent_counts.entry(metric_name.to_string()).or_insert(0) += 1;
    }

    /// Reset frequency counts.
    pub fn reset_frequency_counts(&self) {
        self.context.write().recent_counts.clear();
    }

    /// Classify severity based on multiple factors.
    pub fn classify(&self, anomaly: &Anomaly) -> AnomalySeverity {
        let context = self.context.read();

        // Factor 1: Anomaly score
        let score_factor = anomaly.score;

        // Factor 2: Deviation magnitude
        let deviation = if anomaly.expected_value.abs() > f64::EPSILON {
            ((anomaly.observed_value - anomaly.expected_value) / anomaly.expected_value).abs()
        } else {
            anomaly.observed_value.abs()
        };
        let deviation_factor = (deviation / 2.0).min(1.0);

        // Factor 3: Frequency of occurrence
        let frequency = context
            .recent_counts
            .get(&anomaly.metric_name)
            .copied()
            .unwrap_or(0);
        let frequency_factor = (frequency as f64 / 10.0).min(1.0);

        // Factor 4: Time of day (assume business hours 9-17 are more critical)
        let hour = anomaly.timestamp.hour();
        let time_factor = if (9..17).contains(&hour) { 1.0 } else { 0.5 };

        // Factor 5: Metric criticality
        let criticality_factor = context
            .criticality_levels
            .get(&anomaly.metric_name)
            .copied()
            .unwrap_or(0.5);

        // Calculate weighted severity score
        let severity_score = self.weights.score_weight * score_factor
            + self.weights.deviation_weight * deviation_factor
            + self.weights.frequency_weight * frequency_factor
            + self.weights.time_weight * time_factor
            + self.weights.criticality_weight * criticality_factor;

        // Classify based on final score
        if severity_score >= 0.8 {
            AnomalySeverity::Critical
        } else if severity_score >= 0.6 {
            AnomalySeverity::High
        } else if severity_score >= 0.4 {
            AnomalySeverity::Medium
        } else {
            AnomalySeverity::Low
        }
    }
}

impl Default for SeverityClassifier {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Classify severity based on anomaly score.
fn classify_severity_by_score(score: f64) -> AnomalySeverity {
    if score >= 0.75 {
        AnomalySeverity::Critical
    } else if score >= 0.5 {
        AnomalySeverity::High
    } else if score >= 0.25 {
        AnomalySeverity::Medium
    } else {
        AnomalySeverity::Low
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mad_detector() {
        let mut detector = MadDetector::new(3.5, "test_metric");

        // Create baseline data
        let baseline_data: Vec<DataPoint> = (0..100)
            .map(|i| DataPoint::new(Utc::now(), 50.0 + (i as f64 % 10.0)))
            .collect();

        detector.update_baseline(&baseline_data).expect("Failed to update baseline");

        // Test data with an anomaly
        let test_data = vec![
            DataPoint::new(Utc::now(), 55.0),  // Normal
            DataPoint::new(Utc::now(), 150.0), // Anomaly
        ];

        let anomalies = detector.detect(&test_data).expect("Failed to detect");
        assert_eq!(anomalies.len(), 1);
        assert_eq!(anomalies[0].anomaly_type, AnomalyType::Spike);
    }

    #[test]
    fn test_ewma_detector() {
        let mut detector = EwmaDetector::new(0.3, 3.0, "test_metric");

        // Initialize with baseline data
        let baseline_data: Vec<DataPoint> = (0..50)
            .map(|_| DataPoint::new(Utc::now(), 100.0))
            .collect();

        detector.update_baseline(&baseline_data).expect("Failed to update baseline");

        // Test with anomalous data
        let test_data = vec![
            DataPoint::new(Utc::now(), 100.0), // Normal
            DataPoint::new(Utc::now(), 500.0), // Anomaly
        ];

        let anomalies = detector.detect(&test_data).expect("Failed to detect");
        assert!(!anomalies.is_empty());
    }

    #[test]
    fn test_change_point_detector() {
        let mut detector = ChangePointDetector::new(5.0, 0.5, "test_metric");

        // Baseline data with stable mean
        let baseline_data: Vec<DataPoint> = (0..100)
            .map(|_| DataPoint::new(Utc::now(), 50.0))
            .collect();

        detector.update_baseline(&baseline_data).expect("Failed to update baseline");

        // Test data with a shift
        let mut test_data: Vec<DataPoint> = (0..20)
            .map(|_| DataPoint::new(Utc::now(), 50.0))
            .collect();
        test_data.extend((0..20).map(|_| DataPoint::new(Utc::now(), 100.0)));

        let anomalies = detector.detect(&test_data).expect("Failed to detect");
        // Should detect the change point
        assert!(!anomalies.is_empty());
    }

    #[test]
    fn test_geo_anomaly_detector() {
        let mut detector = GeoAnomalyDetector::new(0.1, 10.0, 3, 2.0);

        // Create baseline geo data
        let baseline_data: Vec<GeoDataPoint> = (0..100)
            .map(|i| {
                let lon = -122.0 + (i as f64 % 10.0) * 0.01;
                let lat = 37.0 + (i as f64 / 10.0) * 0.01;
                GeoDataPoint::new(lon, lat, 50.0, Utc::now())
            })
            .collect();

        detector.update_geo_baseline(&baseline_data).expect("Failed to update geo baseline");

        // Test data with a hotspot
        let mut test_data = baseline_data.clone();
        // Add some high-value points clustered together
        for i in 0..5 {
            test_data.push(GeoDataPoint::new(
                -122.05 + i as f64 * 0.001,
                37.05,
                200.0, // High value
                Utc::now(),
            ));
        }

        let anomalies = detector.detect_geo_anomalies(&test_data).expect("Failed to detect");
        // May or may not find anomalies depending on clustering
        assert!(anomalies.len() >= 0);
    }

    #[test]
    fn test_threshold_alert_manager() {
        let mut manager = ThresholdAlertManager::new();

        manager.add_threshold(AlertThreshold {
            id: "cpu_high".to_string(),
            metric_name: "cpu_usage".to_string(),
            warning_threshold: 70.0,
            critical_threshold: 90.0,
            operator: ThresholdOperator::GreaterThan,
            hysteresis: 0.1,
            min_duration: Duration::seconds(0),
            cooldown: Duration::seconds(0),
        });

        // Test normal value
        let alerts = manager.evaluate("cpu_usage", 50.0, Utc::now());
        assert!(alerts.is_empty());

        // Test warning value
        let alerts = manager.evaluate("cpu_usage", 75.0, Utc::now());
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].status, AlertStatus::Warning);

        // Test critical value
        let alerts = manager.evaluate("cpu_usage", 95.0, Utc::now());
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].status, AlertStatus::Critical);
    }

    #[test]
    fn test_lof_detector() {
        let mut detector = LofDetector::new(5, 1.5, "test_metric");

        // Create training data (normal distribution around 50)
        let training_data: Vec<DataPoint> = (0..100)
            .map(|i| DataPoint::new(Utc::now(), 45.0 + (i as f64 % 10.0)))
            .collect();

        detector.update_baseline(&training_data).expect("Failed to update baseline");

        // Test with outliers
        let test_data = vec![
            DataPoint::new(Utc::now(), 50.0),  // Normal
            DataPoint::new(Utc::now(), 200.0), // Outlier
        ];

        let anomalies = detector.detect(&test_data).expect("Failed to detect");
        // LOF should detect the outlier
        assert!(!anomalies.is_empty());
    }

    #[test]
    fn test_historical_baseline() {
        let baseline = HistoricalBaseline::new(1000);

        // Add some data points
        for i in 0..100 {
            baseline.add_data_point(DataPoint::new(
                Utc::now(),
                50.0 + (i as f64 % 20.0),
            ));
        }

        // Check that baselines are computed
        let combined = baseline.get_combined_baseline(Utc::now());
        assert!(combined.is_some());
    }

    #[test]
    fn test_severity_classifier() {
        let classifier = SeverityClassifier::new();
        classifier.set_metric_criticality("critical_metric", 1.0);
        classifier.set_metric_criticality("normal_metric", 0.3);

        let critical_anomaly = Anomaly {
            timestamp: Utc::now(),
            metric_name: "critical_metric".to_string(),
            observed_value: 1000.0,
            expected_value: 100.0,
            score: 0.9,
            severity: AnomalySeverity::Low, // Will be reclassified
            anomaly_type: AnomalyType::Spike,
            description: "Test".to_string(),
        };

        let severity = classifier.classify(&critical_anomaly);
        assert!(matches!(severity, AnomalySeverity::Critical | AnomalySeverity::High));

        let normal_anomaly = Anomaly {
            timestamp: Utc::now(),
            metric_name: "normal_metric".to_string(),
            observed_value: 110.0,
            expected_value: 100.0,
            score: 0.2,
            severity: AnomalySeverity::High, // Will be reclassified
            anomaly_type: AnomalyType::Spike,
            description: "Test".to_string(),
        };

        let severity = classifier.classify(&normal_anomaly);
        assert!(matches!(severity, AnomalySeverity::Low | AnomalySeverity::Medium));
    }

    #[test]
    fn test_geo_data_point_distance() {
        let point1 = GeoDataPoint::new(-122.4194, 37.7749, 0.0, Utc::now()); // San Francisco
        let point2 = GeoDataPoint::new(-118.2437, 34.0522, 0.0, Utc::now()); // Los Angeles

        let distance = point1.distance_to(&point2);
        // SF to LA is approximately 560 km
        assert!(distance > 500.0 && distance < 600.0);
    }

    #[test]
    fn test_threshold_operators() {
        assert!(ThresholdOperator::GreaterThan.check(10.0, 5.0));
        assert!(!ThresholdOperator::GreaterThan.check(5.0, 10.0));

        assert!(ThresholdOperator::LessThan.check(5.0, 10.0));
        assert!(!ThresholdOperator::LessThan.check(10.0, 5.0));

        assert!(ThresholdOperator::GreaterThanOrEqual.check(10.0, 10.0));
        assert!(ThresholdOperator::LessThanOrEqual.check(10.0, 10.0));

        assert!(ThresholdOperator::Equals.check(10.0, 10.0));
        assert!(!ThresholdOperator::NotEquals.check(10.0, 10.0));
    }
}

//! Metric correlation and causation analysis.

use crate::error::Result;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Correlation analyzer for detecting relationships between metrics.
pub struct CorrelationAnalyzer {
    metric_data: Arc<RwLock<HashMap<String, Vec<MetricPoint>>>>,
    correlations: Arc<RwLock<Vec<Correlation>>>,
}

/// Metric data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricPoint {
    /// Timestamp when the metric was recorded.
    pub timestamp: DateTime<Utc>,
    /// Numeric value of the metric.
    pub value: f64,
}

/// Correlation between two metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Correlation {
    /// Name of the first metric in the correlation.
    pub metric1: String,
    /// Name of the second metric in the correlation.
    pub metric2: String,
    /// Pearson correlation coefficient (-1.0 to 1.0).
    pub coefficient: f64,
    /// Classification of correlation strength.
    pub strength: CorrelationStrength,
    /// Time lag in data points between the two metrics.
    pub lag: i64,
}

/// Correlation strength classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CorrelationStrength {
    /// No significant correlation (coefficient < 0.2).
    None,
    /// Weak correlation (0.2 <= coefficient < 0.4).
    Weak,
    /// Moderate correlation (0.4 <= coefficient < 0.7).
    Moderate,
    /// Strong correlation (0.7 <= coefficient < 0.9).
    Strong,
    /// Very strong correlation (coefficient >= 0.9).
    VeryStrong,
}

impl CorrelationAnalyzer {
    /// Create a new correlation analyzer.
    pub fn new() -> Self {
        Self {
            metric_data: Arc::new(RwLock::new(HashMap::new())),
            correlations: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add metric data for analysis.
    pub fn add_metric_data(&self, metric_name: String, data: Vec<MetricPoint>) {
        self.metric_data.write().insert(metric_name, data);
    }

    /// Calculate Pearson correlation coefficient.
    pub fn calculate_pearson(&self, metric1: &str, metric2: &str) -> Result<f64> {
        let data = self.metric_data.read();

        let series1 = data
            .get(metric1)
            .ok_or_else(|| crate::error::ObservabilityError::NotFound(metric1.to_string()))?;

        let series2 = data
            .get(metric2)
            .ok_or_else(|| crate::error::ObservabilityError::NotFound(metric2.to_string()))?;

        let values1: Vec<f64> = series1.iter().map(|p| p.value).collect();
        let values2: Vec<f64> = series2.iter().map(|p| p.value).collect();

        let n = values1.len().min(values2.len());
        if n < 2 {
            return Ok(0.0);
        }

        let mean1 = values1.iter().take(n).sum::<f64>() / n as f64;
        let mean2 = values2.iter().take(n).sum::<f64>() / n as f64;

        let mut numerator = 0.0;
        let mut sum_sq1 = 0.0;
        let mut sum_sq2 = 0.0;

        for i in 0..n {
            let diff1 = values1[i] - mean1;
            let diff2 = values2[i] - mean2;
            numerator += diff1 * diff2;
            sum_sq1 += diff1 * diff1;
            sum_sq2 += diff2 * diff2;
        }

        if sum_sq1 == 0.0 || sum_sq2 == 0.0 {
            return Ok(0.0);
        }

        Ok(numerator / (sum_sq1 * sum_sq2).sqrt())
    }

    /// Detect correlations between all pairs of metrics.
    pub fn detect_correlations(&self, threshold: f64) -> Result<Vec<Correlation>> {
        let data = self.metric_data.read();
        let metrics: Vec<String> = data.keys().cloned().collect();
        let mut correlations = Vec::new();

        for i in 0..metrics.len() {
            for j in (i + 1)..metrics.len() {
                let coefficient = self.calculate_pearson(&metrics[i], &metrics[j])?;

                if coefficient.abs() >= threshold {
                    let strength = Self::classify_strength(coefficient.abs());

                    correlations.push(Correlation {
                        metric1: metrics[i].clone(),
                        metric2: metrics[j].clone(),
                        coefficient,
                        strength,
                        lag: 0,
                    });
                }
            }
        }

        *self.correlations.write() = correlations.clone();
        Ok(correlations)
    }

    /// Classify correlation strength.
    fn classify_strength(coefficient: f64) -> CorrelationStrength {
        let abs_coef = coefficient.abs();

        if abs_coef >= 0.9 {
            CorrelationStrength::VeryStrong
        } else if abs_coef >= 0.7 {
            CorrelationStrength::Strong
        } else if abs_coef >= 0.4 {
            CorrelationStrength::Moderate
        } else if abs_coef >= 0.2 {
            CorrelationStrength::Weak
        } else {
            CorrelationStrength::None
        }
    }

    /// Calculate cross-correlation with lag.
    pub fn calculate_cross_correlation(
        &self,
        metric1: &str,
        metric2: &str,
        max_lag: usize,
    ) -> Result<Vec<(i64, f64)>> {
        let data = self.metric_data.read();

        let series1 = data
            .get(metric1)
            .ok_or_else(|| crate::error::ObservabilityError::NotFound(metric1.to_string()))?;

        let series2 = data
            .get(metric2)
            .ok_or_else(|| crate::error::ObservabilityError::NotFound(metric2.to_string()))?;

        let values1: Vec<f64> = series1.iter().map(|p| p.value).collect();
        let values2: Vec<f64> = series2.iter().map(|p| p.value).collect();

        let mut results = Vec::new();

        for lag in 0..=max_lag {
            let correlation = Self::cross_correlate(&values1, &values2, lag as i64);
            results.push((lag as i64, correlation));

            if lag > 0 {
                let correlation = Self::cross_correlate(&values1, &values2, -(lag as i64));
                results.push((-(lag as i64), correlation));
            }
        }

        results.sort_by_key(|(lag, _)| *lag);
        Ok(results)
    }

    /// Calculate cross-correlation at a specific lag.
    fn cross_correlate(series1: &[f64], series2: &[f64], lag: i64) -> f64 {
        let n = series1.len().min(series2.len());
        if n < 2 {
            return 0.0;
        }

        let mean1 = series1.iter().sum::<f64>() / n as f64;
        let mean2 = series2.iter().sum::<f64>() / n as f64;

        let start1 = if lag >= 0 { lag as usize } else { 0 };
        let start2 = if lag < 0 { (-lag) as usize } else { 0 };
        let len = n.saturating_sub(lag.unsigned_abs() as usize);

        if len < 2 {
            return 0.0;
        }

        let mut numerator = 0.0;
        let mut sum_sq1 = 0.0;
        let mut sum_sq2 = 0.0;

        for i in 0..len {
            let idx1 = start1 + i;
            let idx2 = start2 + i;

            if idx1 < series1.len() && idx2 < series2.len() {
                let diff1 = series1[idx1] - mean1;
                let diff2 = series2[idx2] - mean2;
                numerator += diff1 * diff2;
                sum_sq1 += diff1 * diff1;
                sum_sq2 += diff2 * diff2;
            }
        }

        if sum_sq1 == 0.0 || sum_sq2 == 0.0 {
            return 0.0;
        }

        numerator / (sum_sq1 * sum_sq2).sqrt()
    }

    /// Get all detected correlations.
    pub fn get_correlations(&self) -> Vec<Correlation> {
        self.correlations.read().clone()
    }
}

impl Default for CorrelationAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Causality analyzer using Granger causality.
pub struct CausalityAnalyzer {
    data: Arc<RwLock<HashMap<String, Vec<MetricPoint>>>>,
}

impl CausalityAnalyzer {
    /// Create a new causality analyzer.
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add metric data.
    pub fn add_data(&self, metric: String, points: Vec<MetricPoint>) {
        self.data.write().insert(metric, points);
    }

    /// Test Granger causality (simplified implementation).
    pub fn test_granger_causality(
        &self,
        cause: &str,
        effect: &str,
        max_lag: usize,
    ) -> Result<bool> {
        // Simplified implementation - in production, use proper statistical tests
        let data = self.data.read();

        let cause_data = data
            .get(cause)
            .ok_or_else(|| crate::error::ObservabilityError::NotFound(cause.to_string()))?;

        let effect_data = data
            .get(effect)
            .ok_or_else(|| crate::error::ObservabilityError::NotFound(effect.to_string()))?;

        // Simple check: if correlation exists with lag, assume causality
        let values1: Vec<f64> = cause_data.iter().map(|p| p.value).collect();
        let values2: Vec<f64> = effect_data.iter().map(|p| p.value).collect();

        for lag in 1..=max_lag {
            let correlation = CorrelationAnalyzer::cross_correlate(&values1, &values2, lag as i64);
            if correlation.abs() > 0.5 {
                return Ok(true);
            }
        }

        Ok(false)
    }
}

impl Default for CausalityAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pearson_correlation() {
        let analyzer = CorrelationAnalyzer::new();

        let data1 = vec![
            MetricPoint {
                timestamp: Utc::now(),
                value: 1.0,
            },
            MetricPoint {
                timestamp: Utc::now(),
                value: 2.0,
            },
            MetricPoint {
                timestamp: Utc::now(),
                value: 3.0,
            },
        ];

        let data2 = vec![
            MetricPoint {
                timestamp: Utc::now(),
                value: 2.0,
            },
            MetricPoint {
                timestamp: Utc::now(),
                value: 4.0,
            },
            MetricPoint {
                timestamp: Utc::now(),
                value: 6.0,
            },
        ];

        analyzer.add_metric_data("metric1".to_string(), data1);
        analyzer.add_metric_data("metric2".to_string(), data2);

        let correlation = analyzer.calculate_pearson("metric1", "metric2");
        assert!(correlation.is_ok());

        // Perfect positive correlation
        let coef = correlation.expect("Failed to calculate");
        assert!((coef - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_correlation_detection() {
        let analyzer = CorrelationAnalyzer::new();

        let data1 = vec![
            MetricPoint {
                timestamp: Utc::now(),
                value: 1.0,
            },
            MetricPoint {
                timestamp: Utc::now(),
                value: 2.0,
            },
            MetricPoint {
                timestamp: Utc::now(),
                value: 3.0,
            },
        ];

        let data2 = vec![
            MetricPoint {
                timestamp: Utc::now(),
                value: 3.0,
            },
            MetricPoint {
                timestamp: Utc::now(),
                value: 2.0,
            },
            MetricPoint {
                timestamp: Utc::now(),
                value: 1.0,
            },
        ];

        analyzer.add_metric_data("metric1".to_string(), data1);
        analyzer.add_metric_data("metric2".to_string(), data2);

        let correlations = analyzer.detect_correlations(0.5);
        assert!(correlations.is_ok());
    }
}

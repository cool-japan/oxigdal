//! Metrics collection for streaming operations.

use crate::error::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Type of metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricType {
    /// Counter metric (monotonically increasing)
    Counter,

    /// Gauge metric (can increase or decrease)
    Gauge,

    /// Histogram metric
    Histogram,

    /// Summary metric
    Summary,

    /// Timer metric
    Timer,
}

/// Value of a metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricValue {
    /// Integer value
    Integer(i64),

    /// Floating point value
    Float(f64),

    /// Histogram values
    Histogram {
        /// Bucket boundaries
        buckets: Vec<f64>,
        /// Counts per bucket
        counts: Vec<u64>,
    },

    /// Summary values
    Summary {
        /// Count
        count: u64,
        /// Sum
        sum: f64,
        /// Quantiles (as sorted vec of (quantile, value) pairs)
        quantiles: Vec<(f64, f64)>,
    },
}

impl MetricValue {
    /// Get as integer.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            MetricValue::Integer(v) => Some(*v),
            _ => None,
        }
    }

    /// Get as float.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            MetricValue::Float(v) => Some(*v),
            MetricValue::Integer(v) => Some(*v as f64),
            _ => None,
        }
    }
}

/// A single metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    /// Metric name
    pub name: String,

    /// Metric type
    pub metric_type: MetricType,

    /// Metric value
    pub value: MetricValue,

    /// Timestamp
    pub timestamp: DateTime<Utc>,

    /// Tags
    pub tags: HashMap<String, String>,

    /// Help text
    pub help: Option<String>,
}

impl Metric {
    /// Create a new metric.
    pub fn new(name: String, metric_type: MetricType, value: MetricValue) -> Self {
        Self {
            name,
            metric_type,
            value,
            timestamp: Utc::now(),
            tags: HashMap::new(),
            help: None,
        }
    }

    /// Add a tag.
    pub fn with_tag(mut self, key: String, value: String) -> Self {
        self.tags.insert(key, value);
        self
    }

    /// Add help text.
    pub fn with_help(mut self, help: String) -> Self {
        self.help = Some(help);
        self
    }
}

/// Collector for streaming metrics.
pub struct MetricsCollector {
    metrics: Arc<RwLock<HashMap<String, Metric>>>,
    enabled: Arc<RwLock<bool>>,
}

impl MetricsCollector {
    /// Create a new metrics collector.
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(HashMap::new())),
            enabled: Arc::new(RwLock::new(true)),
        }
    }

    /// Enable metrics collection.
    pub async fn enable(&self) {
        *self.enabled.write().await = true;
    }

    /// Disable metrics collection.
    pub async fn disable(&self) {
        *self.enabled.write().await = false;
    }

    /// Check if enabled.
    pub async fn is_enabled(&self) -> bool {
        *self.enabled.read().await
    }

    /// Record a metric.
    pub async fn record(&self, metric: Metric) -> Result<()> {
        if !self.is_enabled().await {
            return Ok(());
        }

        let mut metrics = self.metrics.write().await;
        metrics.insert(metric.name.clone(), metric);

        Ok(())
    }

    /// Increment a counter.
    pub async fn increment_counter(&self, name: &str, value: i64) -> Result<()> {
        if !self.is_enabled().await {
            return Ok(());
        }

        let mut metrics = self.metrics.write().await;

        let metric = metrics.entry(name.to_string()).or_insert_with(|| {
            Metric::new(
                name.to_string(),
                MetricType::Counter,
                MetricValue::Integer(0),
            )
        });

        if let MetricValue::Integer(current) = metric.value {
            metric.value = MetricValue::Integer(current + value);
            metric.timestamp = Utc::now();
        }

        Ok(())
    }

    /// Set a gauge value.
    pub async fn set_gauge(&self, name: &str, value: f64) -> Result<()> {
        if !self.is_enabled().await {
            return Ok(());
        }

        let mut metrics = self.metrics.write().await;

        let metric = metrics.entry(name.to_string()).or_insert_with(|| {
            Metric::new(name.to_string(), MetricType::Gauge, MetricValue::Float(0.0))
        });

        metric.value = MetricValue::Float(value);
        metric.timestamp = Utc::now();

        Ok(())
    }

    /// Record a histogram value.
    pub async fn record_histogram(&self, name: &str, value: f64, buckets: Vec<f64>) -> Result<()> {
        if !self.is_enabled().await {
            return Ok(());
        }

        let mut metrics = self.metrics.write().await;

        let metric = metrics.entry(name.to_string()).or_insert_with(|| {
            let counts = vec![0; buckets.len()];
            Metric::new(
                name.to_string(),
                MetricType::Histogram,
                MetricValue::Histogram {
                    buckets: buckets.clone(),
                    counts,
                },
            )
        });

        if let MetricValue::Histogram { buckets, counts } = &mut metric.value {
            for (i, &bucket) in buckets.iter().enumerate() {
                if value <= bucket {
                    counts[i] += 1;
                }
            }
            metric.timestamp = Utc::now();
        }

        Ok(())
    }

    /// Get a metric by name.
    pub async fn get_metric(&self, name: &str) -> Option<Metric> {
        self.metrics.read().await.get(name).cloned()
    }

    /// Get all metrics.
    pub async fn get_all_metrics(&self) -> Vec<Metric> {
        self.metrics.read().await.values().cloned().collect()
    }

    /// Clear all metrics.
    pub async fn clear(&self) -> Result<()> {
        self.metrics.write().await.clear();
        Ok(())
    }

    /// Get metric count.
    pub async fn metric_count(&self) -> usize {
        self.metrics.read().await.len()
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metrics_collector() {
        let collector = MetricsCollector::new();

        assert!(collector.is_enabled().await);
        assert_eq!(collector.metric_count().await, 0);
    }

    #[tokio::test]
    async fn test_counter_metric() {
        let collector = MetricsCollector::new();

        collector
            .increment_counter("test_counter", 5)
            .await
            .expect("Failed to increment counter by 5 in test");
        collector
            .increment_counter("test_counter", 3)
            .await
            .expect("Failed to increment counter by 3 in test");

        let metric = collector
            .get_metric("test_counter")
            .await
            .expect("Failed to get test_counter metric");
        assert_eq!(metric.value.as_i64(), Some(8));
    }

    #[tokio::test]
    async fn test_gauge_metric() {
        let collector = MetricsCollector::new();

        collector
            .set_gauge("test_gauge", 42.5)
            .await
            .expect("Failed to set gauge to 42.5 in test");

        let metric = collector
            .get_metric("test_gauge")
            .await
            .expect("Failed to get test_gauge metric after first set");
        assert_eq!(metric.value.as_f64(), Some(42.5));

        collector
            .set_gauge("test_gauge", 100.0)
            .await
            .expect("Failed to set gauge to 100.0 in test");

        let metric = collector
            .get_metric("test_gauge")
            .await
            .expect("Failed to get test_gauge metric after second set");
        assert_eq!(metric.value.as_f64(), Some(100.0));
    }

    #[tokio::test]
    async fn test_histogram_metric() {
        let collector = MetricsCollector::new();
        let buckets = vec![1.0, 5.0, 10.0, 50.0, 100.0];

        collector
            .record_histogram("test_histogram", 3.0, buckets.clone())
            .await
            .expect("Failed to record histogram value 3.0 in test");

        collector
            .record_histogram("test_histogram", 7.0, buckets.clone())
            .await
            .expect("Failed to record histogram value 7.0 in test");

        let metric = collector
            .get_metric("test_histogram")
            .await
            .expect("Failed to get test_histogram metric");
        assert_eq!(metric.metric_type, MetricType::Histogram);
    }

    #[tokio::test]
    async fn test_enable_disable() {
        let collector = MetricsCollector::new();

        assert!(collector.is_enabled().await);

        collector.disable().await;
        assert!(!collector.is_enabled().await);

        collector
            .increment_counter("test", 1)
            .await
            .expect("Failed to increment counter while disabled in test");
        assert_eq!(collector.metric_count().await, 0);

        collector.enable().await;
        collector
            .increment_counter("test", 1)
            .await
            .expect("Failed to increment counter after enable in test");
        assert_eq!(collector.metric_count().await, 1);
    }
}

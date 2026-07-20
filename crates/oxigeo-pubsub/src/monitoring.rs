//! Monitoring and metrics for Google Cloud Pub/Sub.
//!
//! This module provides integration with Google Cloud Monitoring for
//! tracking Pub/Sub metrics, performance, and health status.

#[cfg(feature = "monitoring")]
use crate::error::Result;
#[cfg(feature = "monitoring")]
use chrono::{DateTime, Utc};
#[cfg(feature = "monitoring")]
use parking_lot::RwLock;
#[cfg(feature = "monitoring")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "monitoring")]
use std::collections::HashMap;
#[cfg(feature = "monitoring")]
use std::sync::Arc;
#[cfg(feature = "monitoring")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "monitoring")]
use std::time::{Duration, Instant};
#[cfg(feature = "monitoring")]
use tracing::{debug, info};

/// Metric type.
#[cfg(feature = "monitoring")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricType {
    /// Counter metric (monotonically increasing).
    Counter,
    /// Gauge metric (can go up and down).
    Gauge,
    /// Histogram metric (distribution of values).
    Histogram,
}

/// Metric value.
#[cfg(feature = "monitoring")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricValue {
    /// Integer value.
    Int(i64),
    /// Floating point value.
    Float(f64),
    /// Distribution (for histograms).
    Distribution {
        /// Count of samples.
        count: u64,
        /// Sum of samples.
        sum: f64,
        /// Minimum value.
        min: f64,
        /// Maximum value.
        max: f64,
        /// Mean value.
        mean: f64,
    },
}

/// Metric data point.
#[cfg(feature = "monitoring")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricPoint {
    /// Metric name.
    pub name: String,
    /// Metric type.
    pub metric_type: MetricType,
    /// Metric value.
    pub value: MetricValue,
    /// Metric labels.
    pub labels: HashMap<String, String>,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
}

#[cfg(feature = "monitoring")]
impl MetricPoint {
    /// Creates a new metric point.
    pub fn new(name: impl Into<String>, metric_type: MetricType, value: MetricValue) -> Self {
        Self {
            name: name.into(),
            metric_type,
            value,
            labels: HashMap::new(),
            timestamp: Utc::now(),
        }
    }

    /// Adds a label to the metric.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Adds multiple labels to the metric.
    pub fn with_labels(mut self, labels: HashMap<String, String>) -> Self {
        self.labels.extend(labels);
        self
    }
}

/// Publisher metrics.
#[cfg(feature = "monitoring")]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublisherMetrics {
    /// Total messages published.
    pub messages_published: u64,
    /// Total bytes published.
    pub bytes_published: u64,
    /// Failed publish operations.
    pub publish_failures: u64,
    /// Average publish latency in milliseconds.
    pub avg_publish_latency_ms: f64,
    /// Maximum publish latency in milliseconds.
    pub max_publish_latency_ms: f64,
    /// Minimum publish latency in milliseconds.
    pub min_publish_latency_ms: f64,
    /// Messages in flight.
    pub messages_in_flight: u64,
    /// Batch publish count.
    pub batch_publishes: u64,
    /// Average batch size.
    pub avg_batch_size: f64,
}

/// Subscriber metrics.
#[cfg(feature = "monitoring")]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubscriberMetrics {
    /// Total messages received.
    pub messages_received: u64,
    /// Total bytes received.
    pub bytes_received: u64,
    /// Messages acknowledged.
    pub messages_acknowledged: u64,
    /// Messages not acknowledged.
    pub messages_nacked: u64,
    /// Messages expired.
    pub messages_expired: u64,
    /// Average acknowledgment latency in milliseconds.
    pub avg_ack_latency_ms: f64,
    /// Outstanding messages.
    pub outstanding_messages: u64,
    /// Outstanding bytes.
    pub outstanding_bytes: u64,
    /// Pull request count.
    pub pull_requests: u64,
    /// Average messages per pull.
    pub avg_messages_per_pull: f64,
}

/// Latency tracker for measuring operation latencies.
#[cfg(feature = "monitoring")]
pub struct LatencyTracker {
    count: AtomicU64,
    total_ms: AtomicU64,
    min_ms: RwLock<f64>,
    max_ms: RwLock<f64>,
}

#[cfg(feature = "monitoring")]
impl LatencyTracker {
    /// Creates a new latency tracker.
    pub fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            total_ms: AtomicU64::new(0),
            min_ms: RwLock::new(f64::MAX),
            max_ms: RwLock::new(0.0),
        }
    }

    /// Records a latency measurement.
    pub fn record(&self, duration: Duration) {
        let ms = duration.as_secs_f64() * 1000.0;

        self.count.fetch_add(1, Ordering::Relaxed);
        self.total_ms.fetch_add(ms as u64, Ordering::Relaxed);

        let mut min = self.min_ms.write();
        if ms < *min {
            *min = ms;
        }
        drop(min);

        let mut max = self.max_ms.write();
        if ms > *max {
            *max = ms;
        }
    }

    /// Gets the average latency in milliseconds.
    pub fn avg_ms(&self) -> f64 {
        let count = self.count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        let total = self.total_ms.load(Ordering::Relaxed);
        total as f64 / count as f64
    }

    /// Gets the minimum latency in milliseconds.
    pub fn min_ms(&self) -> f64 {
        let min = *self.min_ms.read();
        if min == f64::MAX { 0.0 } else { min }
    }

    /// Gets the maximum latency in milliseconds.
    pub fn max_ms(&self) -> f64 {
        *self.max_ms.read()
    }

    /// Gets the count of recorded latencies.
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Resets the latency tracker.
    pub fn reset(&self) {
        self.count.store(0, Ordering::Relaxed);
        self.total_ms.store(0, Ordering::Relaxed);
        *self.min_ms.write() = f64::MAX;
        *self.max_ms.write() = 0.0;
    }
}

#[cfg(feature = "monitoring")]
impl Default for LatencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Operation timer for measuring operation duration.
#[cfg(feature = "monitoring")]
pub struct OperationTimer {
    start: Instant,
    tracker: Arc<LatencyTracker>,
}

#[cfg(feature = "monitoring")]
impl OperationTimer {
    /// Starts a new operation timer.
    pub fn start(tracker: Arc<LatencyTracker>) -> Self {
        Self {
            start: Instant::now(),
            tracker,
        }
    }

    /// Stops the timer and records the duration.
    pub fn stop(self) {
        let duration = self.start.elapsed();
        self.tracker.record(duration);
    }
}

#[cfg(feature = "monitoring")]
impl Drop for OperationTimer {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        self.tracker.record(duration);
    }
}

/// Metrics collector for Pub/Sub operations.
#[cfg(feature = "monitoring")]
pub struct MetricsCollector {
    project_id: String,
    topic_name: Option<String>,
    subscription_name: Option<String>,
    publisher_metrics: Arc<RwLock<PublisherMetrics>>,
    subscriber_metrics: Arc<RwLock<SubscriberMetrics>>,
    publish_latency: Arc<LatencyTracker>,
    ack_latency: Arc<LatencyTracker>,
    custom_metrics: Arc<RwLock<HashMap<String, MetricPoint>>>,
}

#[cfg(feature = "monitoring")]
impl MetricsCollector {
    /// Creates a new metrics collector.
    pub fn new(project_id: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            topic_name: None,
            subscription_name: None,
            publisher_metrics: Arc::new(RwLock::new(PublisherMetrics::default())),
            subscriber_metrics: Arc::new(RwLock::new(SubscriberMetrics::default())),
            publish_latency: Arc::new(LatencyTracker::new()),
            ack_latency: Arc::new(LatencyTracker::new()),
            custom_metrics: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Sets the topic name.
    pub fn with_topic(mut self, topic_name: impl Into<String>) -> Self {
        self.topic_name = Some(topic_name.into());
        self
    }

    /// Sets the subscription name.
    pub fn with_subscription(mut self, subscription_name: impl Into<String>) -> Self {
        self.subscription_name = Some(subscription_name.into());
        self
    }

    /// Records a message published.
    pub fn record_publish(&self, bytes: u64, success: bool) {
        let mut metrics = self.publisher_metrics.write();
        if success {
            metrics.messages_published += 1;
            metrics.bytes_published += bytes;
        } else {
            metrics.publish_failures += 1;
        }

        metrics.avg_publish_latency_ms = self.publish_latency.avg_ms();
        metrics.max_publish_latency_ms = self.publish_latency.max_ms();
        metrics.min_publish_latency_ms = self.publish_latency.min_ms();
    }

    /// Records a batch publish.
    pub fn record_batch_publish(&self, batch_size: usize) {
        let mut metrics = self.publisher_metrics.write();
        metrics.batch_publishes += 1;
        metrics.messages_published += batch_size as u64;
        let total_batches = metrics.batch_publishes;
        if total_batches > 0 {
            metrics.avg_batch_size = metrics.messages_published as f64 / total_batches as f64;
        }
    }

    /// Records a message received.
    pub fn record_receive(&self, bytes: u64) {
        let mut metrics = self.subscriber_metrics.write();
        metrics.messages_received += 1;
        metrics.bytes_received += bytes;
        metrics.outstanding_messages += 1;
        metrics.outstanding_bytes += bytes;
    }

    /// Records a message acknowledgment.
    pub fn record_ack(&self, bytes: u64, success: bool) {
        let mut metrics = self.subscriber_metrics.write();
        if success {
            metrics.messages_acknowledged += 1;
            metrics.outstanding_messages = metrics.outstanding_messages.saturating_sub(1);
            metrics.outstanding_bytes = metrics.outstanding_bytes.saturating_sub(bytes);
        } else {
            metrics.messages_nacked += 1;
        }

        metrics.avg_ack_latency_ms = self.ack_latency.avg_ms();
    }

    /// Records a pull request.
    pub fn record_pull(&self, messages_received: usize) {
        let mut metrics = self.subscriber_metrics.write();
        metrics.pull_requests += 1;
        metrics.messages_received += messages_received as u64;
        let total_pulls = metrics.pull_requests;
        if total_pulls > 0 {
            metrics.avg_messages_per_pull = metrics.messages_received as f64 / total_pulls as f64;
        }
    }

    /// Starts a publish operation timer.
    pub fn start_publish_timer(&self) -> OperationTimer {
        OperationTimer::start(Arc::clone(&self.publish_latency))
    }

    /// Starts an acknowledgment operation timer.
    pub fn start_ack_timer(&self) -> OperationTimer {
        OperationTimer::start(Arc::clone(&self.ack_latency))
    }

    /// Records a custom metric.
    pub fn record_custom_metric(&self, metric: MetricPoint) {
        let mut metrics = self.custom_metrics.write();
        metrics.insert(metric.name.clone(), metric);
    }

    /// Gets the publisher metrics.
    pub fn publisher_metrics(&self) -> PublisherMetrics {
        self.publisher_metrics.read().clone()
    }

    /// Gets the subscriber metrics.
    pub fn subscriber_metrics(&self) -> SubscriberMetrics {
        self.subscriber_metrics.read().clone()
    }

    /// Gets all custom metrics.
    pub fn custom_metrics(&self) -> HashMap<String, MetricPoint> {
        self.custom_metrics.read().clone()
    }

    /// Exports metrics to a format suitable for Cloud Monitoring.
    pub fn export_metrics(&self) -> Vec<MetricPoint> {
        let mut points = Vec::new();

        let mut labels = HashMap::new();
        labels.insert("project_id".to_string(), self.project_id.clone());
        if let Some(topic) = &self.topic_name {
            labels.insert("topic".to_string(), topic.clone());
        }
        if let Some(subscription) = &self.subscription_name {
            labels.insert("subscription".to_string(), subscription.clone());
        }

        // Publisher metrics
        let pub_metrics = self.publisher_metrics.read();
        points.push(
            MetricPoint::new(
                "pubsub/publisher/messages_published",
                MetricType::Counter,
                MetricValue::Int(pub_metrics.messages_published as i64),
            )
            .with_labels(labels.clone()),
        );
        points.push(
            MetricPoint::new(
                "pubsub/publisher/bytes_published",
                MetricType::Counter,
                MetricValue::Int(pub_metrics.bytes_published as i64),
            )
            .with_labels(labels.clone()),
        );
        points.push(
            MetricPoint::new(
                "pubsub/publisher/publish_failures",
                MetricType::Counter,
                MetricValue::Int(pub_metrics.publish_failures as i64),
            )
            .with_labels(labels.clone()),
        );
        points.push(
            MetricPoint::new(
                "pubsub/publisher/avg_latency_ms",
                MetricType::Gauge,
                MetricValue::Float(pub_metrics.avg_publish_latency_ms),
            )
            .with_labels(labels.clone()),
        );

        // Subscriber metrics
        let sub_metrics = self.subscriber_metrics.read();
        points.push(
            MetricPoint::new(
                "pubsub/subscriber/messages_received",
                MetricType::Counter,
                MetricValue::Int(sub_metrics.messages_received as i64),
            )
            .with_labels(labels.clone()),
        );
        points.push(
            MetricPoint::new(
                "pubsub/subscriber/messages_acknowledged",
                MetricType::Counter,
                MetricValue::Int(sub_metrics.messages_acknowledged as i64),
            )
            .with_labels(labels.clone()),
        );
        points.push(
            MetricPoint::new(
                "pubsub/subscriber/outstanding_messages",
                MetricType::Gauge,
                MetricValue::Int(sub_metrics.outstanding_messages as i64),
            )
            .with_labels(labels.clone()),
        );

        // Add custom metrics
        let custom = self.custom_metrics.read();
        points.extend(custom.values().cloned());

        debug!("Exported {} metric points", points.len());
        points
    }

    /// Resets all metrics.
    pub fn reset(&self) {
        *self.publisher_metrics.write() = PublisherMetrics::default();
        *self.subscriber_metrics.write() = SubscriberMetrics::default();
        self.publish_latency.reset();
        self.ack_latency.reset();
        self.custom_metrics.write().clear();
        info!("Metrics reset");
    }

    /// Gets the project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Gets the topic name.
    pub fn topic_name(&self) -> Option<&str> {
        self.topic_name.as_deref()
    }

    /// Gets the subscription name.
    pub fn subscription_name(&self) -> Option<&str> {
        self.subscription_name.as_deref()
    }
}

/// Metrics exporter for sending metrics to Cloud Monitoring.
#[cfg(feature = "monitoring")]
pub struct MetricsExporter {
    collector: Arc<MetricsCollector>,
    export_interval: Duration,
}

#[cfg(feature = "monitoring")]
impl MetricsExporter {
    /// Creates a new metrics exporter.
    pub fn new(collector: Arc<MetricsCollector>, export_interval: Duration) -> Self {
        Self {
            collector,
            export_interval,
        }
    }

    /// Starts the metrics exporter in the background.
    pub async fn start(&self) -> Result<tokio::task::JoinHandle<()>> {
        let collector = Arc::clone(&self.collector);
        let interval = self.export_interval;

        info!("Starting metrics exporter with interval: {:?}", interval);

        let handle = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            loop {
                interval_timer.tick().await;
                let metrics = collector.export_metrics();
                debug!("Exporting {} metrics", metrics.len());
                // In a real implementation, send metrics to Cloud Monitoring API
            }
        });

        Ok(handle)
    }
}

#[cfg(all(test, feature = "monitoring"))]
mod tests {
    use super::*;

    #[test]
    fn test_metric_point_creation() {
        let point = MetricPoint::new("test_metric", MetricType::Counter, MetricValue::Int(42))
            .with_label("key", "value");

        assert_eq!(point.name, "test_metric");
        assert_eq!(point.metric_type, MetricType::Counter);
        assert_eq!(point.labels.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_latency_tracker() {
        let tracker = LatencyTracker::new();
        assert_eq!(tracker.count(), 0);
        assert_eq!(tracker.avg_ms(), 0.0);

        tracker.record(Duration::from_millis(100));
        assert_eq!(tracker.count(), 1);
        assert_eq!(tracker.avg_ms(), 100.0);
        assert_eq!(tracker.min_ms(), 100.0);
        assert_eq!(tracker.max_ms(), 100.0);

        tracker.record(Duration::from_millis(200));
        assert_eq!(tracker.count(), 2);
        assert_eq!(tracker.avg_ms(), 150.0);
        assert_eq!(tracker.min_ms(), 100.0);
        assert_eq!(tracker.max_ms(), 200.0);

        tracker.reset();
        assert_eq!(tracker.count(), 0);
    }

    #[test]
    fn test_metrics_collector() {
        let collector = MetricsCollector::new("test-project")
            .with_topic("test-topic")
            .with_subscription("test-subscription");

        assert_eq!(collector.project_id(), "test-project");
        assert_eq!(collector.topic_name(), Some("test-topic"));
        assert_eq!(collector.subscription_name(), Some("test-subscription"));

        collector.record_publish(100, true);
        let metrics = collector.publisher_metrics();
        assert_eq!(metrics.messages_published, 1);
        assert_eq!(metrics.bytes_published, 100);

        collector.record_receive(200);
        let metrics = collector.subscriber_metrics();
        assert_eq!(metrics.messages_received, 1);
        assert_eq!(metrics.bytes_received, 200);
    }

    #[test]
    fn test_export_metrics() {
        let collector = MetricsCollector::new("test-project");
        collector.record_publish(100, true);
        collector.record_receive(200);

        let exported = collector.export_metrics();
        assert!(!exported.is_empty());
    }
}

#[cfg(not(feature = "monitoring"))]
mod no_monitoring {
    //! Placeholder module when monitoring feature is disabled.
}

//! Metric exporters for various backends.
//!
//! This module provides a comprehensive metric export infrastructure including:
//! - Multiple backend exporters (Prometheus, InfluxDB, StatsD, CloudWatch, JSON)
//! - Batch export support for efficient metric transmission
//! - Export scheduling with configurable intervals
//! - Custom exporter trait for extensibility
//! - Retry and error handling mechanisms

pub mod cloudwatch;
pub mod influxdb;
pub mod json;
pub mod prometheus;
pub mod statsd;

use crate::error::{ObservabilityError, Result};
use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

// Re-export individual exporters
pub use cloudwatch::CloudWatchExporter;
pub use influxdb::InfluxDbExporter;
pub use json::JsonFileExporter;
pub use prometheus::PrometheusExporter;
pub use statsd::StatsdExporter;

// ============================================================================
// Core Metric Types
// ============================================================================

/// Metric data point with comprehensive metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    /// Metric name (should follow naming conventions like prometheus/openmetrics).
    pub name: String,
    /// Metric value.
    pub value: MetricValue,
    /// Labels/tags for dimensional data.
    pub labels: HashMap<String, String>,
    /// Timestamp when the metric was recorded.
    pub timestamp: DateTime<Utc>,
    /// Optional description of the metric.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional unit of measurement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<MetricUnit>,
}

/// Metric value types supporting various metric kinds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricValue {
    /// Monotonically increasing counter.
    Counter(u64),
    /// Instantaneous gauge value.
    Gauge(f64),
    /// Histogram with bucket boundaries and counts.
    Histogram(HistogramValue),
    /// Summary with quantiles.
    Summary(SummaryValue),
    /// Distribution for statistical analysis.
    Distribution(DistributionValue),
}

/// Histogram value with explicit buckets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramValue {
    /// Bucket boundaries (upper exclusive bounds).
    pub buckets: Vec<f64>,
    /// Count for each bucket.
    pub bucket_counts: Vec<u64>,
    /// Total sum of observed values.
    pub sum: f64,
    /// Total count of observations.
    pub count: u64,
    /// Minimum observed value.
    pub min: Option<f64>,
    /// Maximum observed value.
    pub max: Option<f64>,
}

/// Summary value with quantiles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryValue {
    /// Quantile-value pairs (e.g., 0.5 -> median, 0.99 -> p99).
    pub quantiles: Vec<(f64, f64)>,
    /// Total sum of observed values.
    pub sum: f64,
    /// Total count of observations.
    pub count: u64,
}

/// Distribution value for statistical metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributionValue {
    /// All recorded values (for computation).
    pub values: Vec<f64>,
    /// Pre-computed mean.
    pub mean: f64,
    /// Pre-computed standard deviation.
    pub std_dev: f64,
    /// Pre-computed minimum.
    pub min: f64,
    /// Pre-computed maximum.
    pub max: f64,
}

/// Standard metric units.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MetricUnit {
    /// Bytes.
    Bytes,
    /// Kilobytes.
    Kilobytes,
    /// Megabytes.
    Megabytes,
    /// Gigabytes.
    Gigabytes,
    /// Nanoseconds.
    Nanoseconds,
    /// Microseconds.
    Microseconds,
    /// Milliseconds.
    Milliseconds,
    /// Seconds.
    Seconds,
    /// Count/number.
    Count,
    /// Percentage (0-100).
    Percent,
    /// Ratio (0-1).
    Ratio,
    /// Requests per second.
    RequestsPerSecond,
    /// Custom unit.
    Custom,
}

impl Metric {
    /// Create a new counter metric.
    pub fn counter(name: impl Into<String>, value: u64) -> Self {
        Self {
            name: name.into(),
            value: MetricValue::Counter(value),
            labels: HashMap::new(),
            timestamp: Utc::now(),
            description: None,
            unit: Some(MetricUnit::Count),
        }
    }

    /// Create a new gauge metric.
    pub fn gauge(name: impl Into<String>, value: f64) -> Self {
        Self {
            name: name.into(),
            value: MetricValue::Gauge(value),
            labels: HashMap::new(),
            timestamp: Utc::now(),
            description: None,
            unit: None,
        }
    }

    /// Create a histogram metric with default buckets.
    pub fn histogram(name: impl Into<String>, values: Vec<f64>) -> Self {
        let histogram = HistogramValue::from_values(&values, &default_histogram_buckets());
        Self {
            name: name.into(),
            value: MetricValue::Histogram(histogram),
            labels: HashMap::new(),
            timestamp: Utc::now(),
            description: None,
            unit: None,
        }
    }

    /// Create a histogram metric with custom buckets.
    pub fn histogram_with_buckets(
        name: impl Into<String>,
        values: Vec<f64>,
        buckets: &[f64],
    ) -> Self {
        let histogram = HistogramValue::from_values(&values, buckets);
        Self {
            name: name.into(),
            value: MetricValue::Histogram(histogram),
            labels: HashMap::new(),
            timestamp: Utc::now(),
            description: None,
            unit: None,
        }
    }

    /// Create a summary metric.
    pub fn summary(name: impl Into<String>, values: Vec<f64>) -> Self {
        let summary = SummaryValue::from_values(&values);
        Self {
            name: name.into(),
            value: MetricValue::Summary(summary),
            labels: HashMap::new(),
            timestamp: Utc::now(),
            description: None,
            unit: None,
        }
    }

    /// Create a distribution metric.
    pub fn distribution(name: impl Into<String>, values: Vec<f64>) -> Self {
        let distribution = DistributionValue::from_values(&values);
        Self {
            name: name.into(),
            value: MetricValue::Distribution(distribution),
            labels: HashMap::new(),
            timestamp: Utc::now(),
            description: None,
            unit: None,
        }
    }

    /// Add a label to the metric.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Add multiple labels to the metric.
    pub fn with_labels(mut self, labels: HashMap<String, String>) -> Self {
        self.labels.extend(labels);
        self
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the unit.
    pub fn with_unit(mut self, unit: MetricUnit) -> Self {
        self.unit = Some(unit);
        self
    }

    /// Set a custom timestamp.
    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Get the metric type as a string.
    pub fn metric_type(&self) -> &'static str {
        match &self.value {
            MetricValue::Counter(_) => "counter",
            MetricValue::Gauge(_) => "gauge",
            MetricValue::Histogram(_) => "histogram",
            MetricValue::Summary(_) => "summary",
            MetricValue::Distribution(_) => "distribution",
        }
    }
}

impl HistogramValue {
    /// Create a histogram from raw values and bucket boundaries.
    pub fn from_values(values: &[f64], buckets: &[f64]) -> Self {
        let mut bucket_counts = vec![0u64; buckets.len() + 1];
        let mut sum = 0.0;
        let mut min: Option<f64> = None;
        let mut max: Option<f64> = None;

        for &value in values {
            sum += value;
            min = Some(min.map_or(value, |m| m.min(value)));
            max = Some(max.map_or(value, |m| m.max(value)));

            let mut placed = false;
            for (i, &boundary) in buckets.iter().enumerate() {
                if value <= boundary {
                    bucket_counts[i] += 1;
                    placed = true;
                    break;
                }
            }
            if !placed {
                bucket_counts[buckets.len()] += 1;
            }
        }

        Self {
            buckets: buckets.to_vec(),
            bucket_counts,
            sum,
            count: values.len() as u64,
            min,
            max,
        }
    }
}

impl SummaryValue {
    /// Create a summary from raw values with default quantiles.
    pub fn from_values(values: &[f64]) -> Self {
        Self::from_values_with_quantiles(values, &[0.5, 0.9, 0.95, 0.99])
    }

    /// Create a summary from raw values with custom quantiles.
    pub fn from_values_with_quantiles(values: &[f64], quantile_points: &[f64]) -> Self {
        if values.is_empty() {
            return Self {
                quantiles: quantile_points.iter().map(|&q| (q, 0.0)).collect(),
                sum: 0.0,
                count: 0,
            };
        }

        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let sum: f64 = values.iter().sum();
        let count = values.len();

        let quantiles: Vec<(f64, f64)> = quantile_points
            .iter()
            .map(|&q| {
                let idx = ((q * (count as f64 - 1.0)).floor() as usize).min(count - 1);
                (q, sorted[idx])
            })
            .collect();

        Self {
            quantiles,
            sum,
            count: count as u64,
        }
    }
}

impl DistributionValue {
    /// Create a distribution from raw values.
    pub fn from_values(values: &[f64]) -> Self {
        if values.is_empty() {
            return Self {
                values: Vec::new(),
                mean: 0.0,
                std_dev: 0.0,
                min: 0.0,
                max: 0.0,
            };
        }

        let sum: f64 = values.iter().sum();
        let mean = sum / values.len() as f64;

        let variance: f64 =
            values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
        let std_dev = variance.sqrt();

        let min = values.iter().copied().fold(f64::INFINITY, |a, b| a.min(b));
        let max = values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, |a, b| a.max(b));

        Self {
            values: values.to_vec(),
            mean,
            std_dev,
            min,
            max,
        }
    }
}

/// Default histogram bucket boundaries (Prometheus-style).
pub fn default_histogram_buckets() -> Vec<f64> {
    vec![
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ]
}

/// Latency-focused histogram buckets (in seconds).
pub fn latency_buckets() -> Vec<f64> {
    vec![
        0.001, 0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5, 10.0,
    ]
}

/// Size-focused histogram buckets (in bytes).
pub fn size_buckets() -> Vec<f64> {
    vec![
        1024.0,        // 1KB
        10240.0,       // 10KB
        102400.0,      // 100KB
        1048576.0,     // 1MB
        10485760.0,    // 10MB
        104857600.0,   // 100MB
        1073741824.0,  // 1GB
        10737418240.0, // 10GB
    ]
}

// ============================================================================
// Exporter Traits
// ============================================================================

/// Core trait for synchronous metric exporters.
pub trait MetricExporter: Send + Sync {
    /// Export metrics to the backend.
    fn export(&self, metrics: &[Metric]) -> Result<()>;

    /// Flush any buffered metrics.
    fn flush(&self) -> Result<()>;

    /// Get the exporter name for identification.
    fn name(&self) -> &str;

    /// Check if the exporter is healthy/connected.
    fn is_healthy(&self) -> bool {
        true
    }
}

/// Trait for asynchronous metric exporters.
#[allow(async_fn_in_trait)]
pub trait AsyncMetricExporter: Send + Sync {
    /// Export metrics to the backend asynchronously.
    async fn export_async(&self, metrics: &[Metric]) -> Result<()>;

    /// Flush any buffered metrics asynchronously.
    async fn flush_async(&self) -> Result<()>;

    /// Get the exporter name for identification.
    fn name(&self) -> &str;

    /// Check if the exporter is healthy/connected.
    async fn is_healthy(&self) -> bool {
        true
    }
}

/// Trait for custom exporter implementations.
pub trait CustomExporter: Send + Sync {
    /// Export metrics with custom transformation logic.
    fn export_custom(&self, metrics: &[Metric], context: &ExportContext) -> Result<()>;

    /// Get metadata about the exporter.
    fn metadata(&self) -> ExporterMetadata;

    /// Validate that a metric can be exported by this exporter.
    fn validate_metric(&self, metric: &Metric) -> Result<()> {
        // Default implementation accepts all metrics
        let _ = metric;
        Ok(())
    }

    /// Transform metrics before export (hook for preprocessing).
    fn transform_metrics(&self, metrics: &[Metric]) -> Vec<Metric> {
        metrics.to_vec()
    }
}

/// Context passed to custom exporters during export.
#[derive(Debug, Clone)]
pub struct ExportContext {
    /// Current batch number.
    pub batch_number: u64,
    /// Total metrics in this export session.
    pub total_metrics: usize,
    /// Export start time.
    pub export_start: DateTime<Utc>,
    /// Custom metadata.
    pub metadata: HashMap<String, String>,
}

impl Default for ExportContext {
    fn default() -> Self {
        Self {
            batch_number: 0,
            total_metrics: 0,
            export_start: Utc::now(),
            metadata: HashMap::new(),
        }
    }
}

/// Metadata about an exporter.
#[derive(Debug, Clone)]
pub struct ExporterMetadata {
    /// Exporter name.
    pub name: String,
    /// Exporter version.
    pub version: String,
    /// Supported metric types.
    pub supported_types: Vec<String>,
    /// Maximum batch size (0 = unlimited).
    pub max_batch_size: usize,
    /// Whether the exporter supports async operations.
    pub supports_async: bool,
}

// ============================================================================
// Batch Export Support
// ============================================================================

/// Configuration for batch exports.
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Maximum number of metrics per batch.
    pub max_batch_size: usize,
    /// Maximum time to wait before flushing a partial batch.
    pub max_batch_delay: Duration,
    /// Whether to retry failed batches.
    pub retry_on_failure: bool,
    /// Maximum retry attempts for failed batches.
    pub max_retries: u32,
    /// Delay between retry attempts.
    pub retry_delay: Duration,
    /// Whether to drop metrics when queue is full.
    pub drop_on_overflow: bool,
    /// Maximum queue size (0 = unlimited).
    pub max_queue_size: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 1000,
            max_batch_delay: Duration::from_secs(10),
            retry_on_failure: true,
            max_retries: 3,
            retry_delay: Duration::from_millis(100),
            drop_on_overflow: false,
            max_queue_size: 10000,
        }
    }
}

/// Batch exporter wrapper that buffers metrics and exports in batches.
pub struct BatchExporter<E: MetricExporter> {
    inner: E,
    config: BatchConfig,
    buffer: Mutex<Vec<Metric>>,
    stats: BatchExportStats,
    last_flush: Mutex<DateTime<Utc>>,
}

/// Statistics for batch export operations.
#[derive(Debug, Default)]
pub struct BatchExportStats {
    /// Total metrics received.
    pub metrics_received: AtomicU64,
    /// Total metrics exported.
    pub metrics_exported: AtomicU64,
    /// Total metrics dropped.
    pub metrics_dropped: AtomicU64,
    /// Total batches exported.
    pub batches_exported: AtomicU64,
    /// Total export failures.
    pub export_failures: AtomicU64,
    /// Total retries.
    pub retries: AtomicU64,
}

impl<E: MetricExporter> BatchExporter<E> {
    /// Create a new batch exporter with default configuration.
    pub fn new(exporter: E) -> Self {
        Self::with_config(exporter, BatchConfig::default())
    }

    /// Create a new batch exporter with custom configuration.
    pub fn with_config(exporter: E, config: BatchConfig) -> Self {
        Self {
            inner: exporter,
            config,
            buffer: Mutex::new(Vec::new()),
            stats: BatchExportStats::default(),
            last_flush: Mutex::new(Utc::now()),
        }
    }

    /// Add metrics to the batch buffer.
    pub fn add_metrics(&self, metrics: &[Metric]) -> Result<()> {
        self.stats
            .metrics_received
            .fetch_add(metrics.len() as u64, Ordering::Relaxed);

        let mut buffer = self.buffer.lock();

        // Check queue size limit
        if self.config.max_queue_size > 0
            && buffer.len() + metrics.len() > self.config.max_queue_size
        {
            if self.config.drop_on_overflow {
                let to_drop = (buffer.len() + metrics.len()) - self.config.max_queue_size;
                self.stats
                    .metrics_dropped
                    .fetch_add(to_drop as u64, Ordering::Relaxed);
                // Only add what fits
                let can_add = self.config.max_queue_size.saturating_sub(buffer.len());
                buffer.extend_from_slice(&metrics[..can_add.min(metrics.len())]);
            } else {
                return Err(ObservabilityError::MetricsExportFailed(
                    "Queue overflow".to_string(),
                ));
            }
        } else {
            buffer.extend_from_slice(metrics);
        }

        // Check if we should flush
        if buffer.len() >= self.config.max_batch_size {
            drop(buffer);
            self.flush_internal()?;
        }

        Ok(())
    }

    /// Force flush all buffered metrics.
    pub fn force_flush(&self) -> Result<()> {
        self.flush_internal()
    }

    /// Check if a flush is due based on time.
    pub fn should_flush(&self) -> bool {
        let last_flush = self.last_flush.lock();
        let elapsed = Utc::now().signed_duration_since(*last_flush);
        elapsed.to_std().unwrap_or(Duration::ZERO) >= self.config.max_batch_delay
    }

    /// Get current buffer size.
    pub fn buffer_size(&self) -> usize {
        self.buffer.lock().len()
    }

    /// Get export statistics.
    pub fn stats(&self) -> &BatchExportStats {
        &self.stats
    }

    fn flush_internal(&self) -> Result<()> {
        let metrics = {
            let mut buffer = self.buffer.lock();
            std::mem::take(&mut *buffer)
        };

        if metrics.is_empty() {
            return Ok(());
        }

        // Export in chunks
        for chunk in metrics.chunks(self.config.max_batch_size) {
            let mut attempts = 0;
            let mut last_error = None;

            while attempts <= self.config.max_retries {
                match self.inner.export(chunk) {
                    Ok(()) => {
                        self.stats
                            .metrics_exported
                            .fetch_add(chunk.len() as u64, Ordering::Relaxed);
                        self.stats.batches_exported.fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                    Err(e) => {
                        last_error = Some(e);
                        attempts += 1;
                        if attempts <= self.config.max_retries && self.config.retry_on_failure {
                            self.stats.retries.fetch_add(1, Ordering::Relaxed);
                            std::thread::sleep(self.config.retry_delay);
                        }
                    }
                }
            }

            if attempts > self.config.max_retries {
                self.stats.export_failures.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .metrics_dropped
                    .fetch_add(chunk.len() as u64, Ordering::Relaxed);
                if let Some(e) = last_error {
                    return Err(e);
                }
            }
        }

        *self.last_flush.lock() = Utc::now();
        Ok(())
    }
}

impl<E: MetricExporter> MetricExporter for BatchExporter<E> {
    fn export(&self, metrics: &[Metric]) -> Result<()> {
        self.add_metrics(metrics)
    }

    fn flush(&self) -> Result<()> {
        self.force_flush()
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn is_healthy(&self) -> bool {
        self.inner.is_healthy()
    }
}

// ============================================================================
// Export Scheduling
// ============================================================================

/// Configuration for scheduled exports.
#[derive(Debug, Clone)]
pub struct ScheduleConfig {
    /// Export interval.
    pub interval: Duration,
    /// Whether to export immediately on start.
    pub export_on_start: bool,
    /// Whether to export on shutdown.
    pub export_on_shutdown: bool,
    /// Maximum metrics per scheduled export.
    pub max_metrics_per_export: usize,
    /// Whether to skip export if no new metrics.
    pub skip_empty_exports: bool,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            export_on_start: false,
            export_on_shutdown: true,
            max_metrics_per_export: 10000,
            skip_empty_exports: true,
        }
    }
}

/// Scheduled exporter that periodically flushes metrics.
pub struct ScheduledExporter<E: MetricExporter + 'static> {
    exporter: Arc<BatchExporter<E>>,
    config: ScheduleConfig,
    running: Arc<AtomicBool>,
    metrics_collector: Arc<RwLock<Vec<Metric>>>,
}

impl<E: MetricExporter + 'static> ScheduledExporter<E> {
    /// Create a new scheduled exporter.
    pub fn new(exporter: E, config: ScheduleConfig) -> Self {
        Self {
            exporter: Arc::new(BatchExporter::new(exporter)),
            config,
            running: Arc::new(AtomicBool::new(false)),
            metrics_collector: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create with custom batch configuration.
    pub fn with_batch_config(
        exporter: E,
        schedule_config: ScheduleConfig,
        batch_config: BatchConfig,
    ) -> Self {
        Self {
            exporter: Arc::new(BatchExporter::with_config(exporter, batch_config)),
            config: schedule_config,
            running: Arc::new(AtomicBool::new(false)),
            metrics_collector: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Record a metric for later export.
    pub fn record(&self, metric: Metric) {
        let mut collector = self.metrics_collector.write();
        if collector.len() < self.config.max_metrics_per_export {
            collector.push(metric);
        }
    }

    /// Record multiple metrics for later export.
    pub fn record_many(&self, metrics: &[Metric]) {
        let mut collector = self.metrics_collector.write();
        let available = self
            .config
            .max_metrics_per_export
            .saturating_sub(collector.len());
        collector.extend_from_slice(&metrics[..available.min(metrics.len())]);
    }

    /// Start the scheduled export background task.
    pub fn start(&self) -> ExportHandle {
        self.running.store(true, Ordering::SeqCst);

        if self.config.export_on_start {
            let _ = self.export_now();
        }

        let running = Arc::clone(&self.running);
        let exporter = Arc::clone(&self.exporter);
        let collector = Arc::clone(&self.metrics_collector);
        let interval = self.config.interval;
        let skip_empty = self.config.skip_empty_exports;

        let handle = std::thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                std::thread::sleep(interval);

                if !running.load(Ordering::SeqCst) {
                    break;
                }

                let metrics = {
                    let mut c = collector.write();
                    std::mem::take(&mut *c)
                };

                if metrics.is_empty() && skip_empty {
                    continue;
                }

                if let Err(e) = exporter.add_metrics(&metrics) {
                    tracing::warn!("Scheduled export failed: {}", e);
                }

                if let Err(e) = exporter.force_flush() {
                    tracing::warn!("Scheduled flush failed: {}", e);
                }
            }
        });

        ExportHandle {
            running: Arc::clone(&self.running),
            handle: Some(handle),
        }
    }

    /// Export all collected metrics immediately.
    pub fn export_now(&self) -> Result<()> {
        let metrics = {
            let mut collector = self.metrics_collector.write();
            std::mem::take(&mut *collector)
        };

        if metrics.is_empty() && self.config.skip_empty_exports {
            return Ok(());
        }

        self.exporter.add_metrics(&metrics)?;
        self.exporter.force_flush()
    }

    /// Stop the scheduled export.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Get the underlying batch exporter.
    pub fn inner(&self) -> &BatchExporter<E> {
        &self.exporter
    }

    /// Check if the scheduler is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl<E: MetricExporter + 'static> Drop for ScheduledExporter<E> {
    fn drop(&mut self) {
        if self.config.export_on_shutdown {
            let _ = self.export_now();
        }
        self.stop();
    }
}

/// Handle for controlling a running export scheduler.
pub struct ExportHandle {
    running: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl ExportHandle {
    /// Stop the export scheduler.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    /// Check if the scheduler is still running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for ExportHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

// ============================================================================
// Multi-Exporter Support
// ============================================================================

/// Fan-out exporter that sends metrics to multiple backends.
pub struct MultiExporter {
    exporters: Vec<Box<dyn MetricExporter>>,
    fail_fast: bool,
}

impl MultiExporter {
    /// Create a new multi-exporter.
    pub fn new() -> Self {
        Self {
            exporters: Vec::new(),
            fail_fast: false,
        }
    }

    /// Add an exporter to the fan-out list.
    pub fn add_exporter<E: MetricExporter + 'static>(mut self, exporter: E) -> Self {
        self.exporters.push(Box::new(exporter));
        self
    }

    /// Set whether to fail fast on first error.
    pub fn fail_fast(mut self, fail_fast: bool) -> Self {
        self.fail_fast = fail_fast;
        self
    }

    /// Get the number of configured exporters.
    pub fn exporter_count(&self) -> usize {
        self.exporters.len()
    }
}

impl Default for MultiExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricExporter for MultiExporter {
    fn export(&self, metrics: &[Metric]) -> Result<()> {
        let mut errors = Vec::new();

        for exporter in &self.exporters {
            if let Err(e) = exporter.export(metrics) {
                if self.fail_fast {
                    return Err(e);
                }
                errors.push(format!("{}: {}", exporter.name(), e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ObservabilityError::MetricsExportFailed(errors.join("; ")))
        }
    }

    fn flush(&self) -> Result<()> {
        let mut errors = Vec::new();

        for exporter in &self.exporters {
            if let Err(e) = exporter.flush() {
                if self.fail_fast {
                    return Err(e);
                }
                errors.push(format!("{}: {}", exporter.name(), e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ObservabilityError::MetricsExportFailed(errors.join("; ")))
        }
    }

    fn name(&self) -> &str {
        "multi-exporter"
    }

    fn is_healthy(&self) -> bool {
        self.exporters.iter().all(|e| e.is_healthy())
    }
}

// ============================================================================
// Filtering Exporter
// ============================================================================

/// Predicate for filtering metrics.
pub trait MetricFilter: Send + Sync {
    /// Return true if the metric should be exported.
    fn should_export(&self, metric: &Metric) -> bool;
}

/// Filter that matches metrics by name prefix.
pub struct PrefixFilter {
    prefixes: Vec<String>,
    allow: bool,
}

impl PrefixFilter {
    /// Create a filter that allows metrics with the given prefixes.
    pub fn allow(prefixes: Vec<String>) -> Self {
        Self {
            prefixes,
            allow: true,
        }
    }

    /// Create a filter that denies metrics with the given prefixes.
    pub fn deny(prefixes: Vec<String>) -> Self {
        Self {
            prefixes,
            allow: false,
        }
    }
}

impl MetricFilter for PrefixFilter {
    fn should_export(&self, metric: &Metric) -> bool {
        let matches = self.prefixes.iter().any(|p| metric.name.starts_with(p));
        if self.allow { matches } else { !matches }
    }
}

/// Filter that matches metrics by label presence.
pub struct LabelFilter {
    required_labels: Vec<String>,
}

impl LabelFilter {
    /// Create a filter requiring specific labels.
    pub fn requiring(labels: Vec<String>) -> Self {
        Self {
            required_labels: labels,
        }
    }
}

impl MetricFilter for LabelFilter {
    fn should_export(&self, metric: &Metric) -> bool {
        self.required_labels
            .iter()
            .all(|l| metric.labels.contains_key(l))
    }
}

/// Exporter wrapper that filters metrics before export.
pub struct FilteredExporter<E: MetricExporter> {
    inner: E,
    filters: Vec<Box<dyn MetricFilter>>,
}

impl<E: MetricExporter> FilteredExporter<E> {
    /// Create a new filtered exporter.
    pub fn new(exporter: E) -> Self {
        Self {
            inner: exporter,
            filters: Vec::new(),
        }
    }

    /// Add a filter to the chain.
    pub fn with_filter<F: MetricFilter + 'static>(mut self, filter: F) -> Self {
        self.filters.push(Box::new(filter));
        self
    }
}

impl<E: MetricExporter> MetricExporter for FilteredExporter<E> {
    fn export(&self, metrics: &[Metric]) -> Result<()> {
        let filtered: Vec<Metric> = metrics
            .iter()
            .filter(|m| self.filters.iter().all(|f| f.should_export(m)))
            .cloned()
            .collect();

        if filtered.is_empty() {
            return Ok(());
        }

        self.inner.export(&filtered)
    }

    fn flush(&self) -> Result<()> {
        self.inner.flush()
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn is_healthy(&self) -> bool {
        self.inner.is_healthy()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    struct TestExporter {
        name: String,
        exported: Arc<Mutex<Vec<Metric>>>,
    }

    impl TestExporter {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                exported: Arc::new(Mutex::new(Vec::new())),
            }
        }

        #[allow(dead_code)]
        fn get_exported(&self) -> Vec<Metric> {
            self.exported.lock().clone()
        }
    }

    impl MetricExporter for TestExporter {
        fn export(&self, metrics: &[Metric]) -> Result<()> {
            self.exported.lock().extend_from_slice(metrics);
            Ok(())
        }

        fn flush(&self) -> Result<()> {
            Ok(())
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    #[test]
    fn test_metric_creation() {
        let counter = Metric::counter("requests_total", 42);
        assert_eq!(counter.name, "requests_total");
        assert!(matches!(counter.value, MetricValue::Counter(42)));

        let gauge = Metric::gauge("temperature", 23.5);
        assert_eq!(gauge.name, "temperature");
        assert!(matches!(gauge.value, MetricValue::Gauge(v) if (v - 23.5).abs() < f64::EPSILON));
    }

    #[test]
    fn test_metric_with_labels() {
        let metric = Metric::counter("requests", 100)
            .with_label("method", "GET")
            .with_label("status", "200");

        assert_eq!(metric.labels.get("method"), Some(&"GET".to_string()));
        assert_eq!(metric.labels.get("status"), Some(&"200".to_string()));
    }

    #[test]
    fn test_histogram_value() {
        let values = vec![0.1, 0.5, 1.0, 2.0, 5.0];
        let buckets = vec![0.5, 1.0, 2.5, 5.0, 10.0];
        let histogram = HistogramValue::from_values(&values, &buckets);

        assert_eq!(histogram.count, 5);
        assert!((histogram.sum - 8.6).abs() < 0.001);
        assert_eq!(histogram.min, Some(0.1));
        assert_eq!(histogram.max, Some(5.0));
    }

    #[test]
    fn test_summary_value() {
        let values: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let summary = SummaryValue::from_values(&values);

        assert_eq!(summary.count, 100);
        assert!((summary.sum - 5050.0).abs() < 0.001);
    }

    #[test]
    fn test_distribution_value() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let dist = DistributionValue::from_values(&values);

        assert!((dist.mean - 3.0).abs() < 0.001);
        assert_eq!(dist.min, 1.0);
        assert_eq!(dist.max, 5.0);
    }

    #[test]
    fn test_batch_exporter() {
        let test_exporter = TestExporter::new("test");
        let exported = Arc::clone(&test_exporter.exported);

        let batch = BatchExporter::with_config(
            test_exporter,
            BatchConfig {
                max_batch_size: 5,
                ..Default::default()
            },
        );

        // Add metrics below threshold
        let metrics: Vec<Metric> = (0..3)
            .map(|i| Metric::counter(format!("metric_{}", i), i as u64))
            .collect();
        batch.add_metrics(&metrics).expect("add should succeed");

        // Buffer should have metrics, but not exported yet
        assert_eq!(batch.buffer_size(), 3);
        assert_eq!(exported.lock().len(), 0);

        // Add more to trigger flush
        let more_metrics: Vec<Metric> = (3..8)
            .map(|i| Metric::counter(format!("metric_{}", i), i as u64))
            .collect();
        batch
            .add_metrics(&more_metrics)
            .expect("add should succeed");

        // Should have exported
        assert!(!exported.lock().is_empty());
    }

    #[test]
    fn test_multi_exporter() {
        let exporter1 = TestExporter::new("exporter1");
        let exporter2 = TestExporter::new("exporter2");
        let exported1 = Arc::clone(&exporter1.exported);
        let exported2 = Arc::clone(&exporter2.exported);

        let multi = MultiExporter::new()
            .add_exporter(exporter1)
            .add_exporter(exporter2);

        let metrics = vec![Metric::counter("test", 1)];
        multi.export(&metrics).expect("export should succeed");

        assert_eq!(exported1.lock().len(), 1);
        assert_eq!(exported2.lock().len(), 1);
    }

    #[test]
    fn test_prefix_filter() {
        let allow_filter = PrefixFilter::allow(vec!["http_".to_string(), "grpc_".to_string()]);

        let http_metric = Metric::counter("http_requests", 100);
        let db_metric = Metric::counter("db_queries", 50);

        assert!(allow_filter.should_export(&http_metric));
        assert!(!allow_filter.should_export(&db_metric));
    }

    #[test]
    fn test_filtered_exporter() {
        let test_exporter = TestExporter::new("filtered");
        let exported = Arc::clone(&test_exporter.exported);

        let filtered = FilteredExporter::new(test_exporter)
            .with_filter(PrefixFilter::allow(vec!["allowed_".to_string()]));

        let metrics = vec![
            Metric::counter("allowed_metric", 1),
            Metric::counter("denied_metric", 2),
        ];

        filtered.export(&metrics).expect("export should succeed");

        let result = exported.lock();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "allowed_metric");
    }
}

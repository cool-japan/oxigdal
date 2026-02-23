//! JSON file exporter for metrics.
//!
//! This module provides a file-based JSON exporter that writes metrics
//! to local files with configurable rotation and formatting options.

use super::{Metric, MetricExporter, MetricValue};
use crate::error::{ObservabilityError, Result};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// Configuration for the JSON file exporter.
#[derive(Debug, Clone)]
pub struct JsonExporterConfig {
    /// Base directory for output files.
    pub output_dir: PathBuf,
    /// File name prefix.
    pub file_prefix: String,
    /// Whether to pretty-print JSON.
    pub pretty_print: bool,
    /// Maximum file size before rotation (in bytes).
    pub max_file_size: u64,
    /// Maximum number of rotated files to keep.
    pub max_files: usize,
    /// Whether to include metadata in output.
    pub include_metadata: bool,
    /// Whether to write metrics as JSON lines (one JSON object per line).
    pub json_lines_format: bool,
    /// Whether to compress rotated files.
    pub compress_rotated: bool,
    /// Custom file extension.
    pub file_extension: String,
}

impl Default for JsonExporterConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("/tmp/oxigdal-metrics"),
            file_prefix: "metrics".to_string(),
            pretty_print: false,
            max_file_size: 100 * 1024 * 1024, // 100MB
            max_files: 10,
            include_metadata: true,
            json_lines_format: true,
            compress_rotated: false,
            file_extension: "json".to_string(),
        }
    }
}

impl JsonExporterConfig {
    /// Create a new configuration with the specified output directory.
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            ..Default::default()
        }
    }

    /// Set the file prefix.
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.file_prefix = prefix.into();
        self
    }

    /// Enable pretty printing.
    pub fn with_pretty_print(mut self, enabled: bool) -> Self {
        self.pretty_print = enabled;
        self
    }

    /// Set maximum file size for rotation.
    pub fn with_max_file_size(mut self, size: u64) -> Self {
        self.max_file_size = size;
        self
    }

    /// Set maximum number of rotated files.
    pub fn with_max_files(mut self, count: usize) -> Self {
        self.max_files = count;
        self
    }

    /// Enable JSON lines format.
    pub fn with_json_lines(mut self, enabled: bool) -> Self {
        self.json_lines_format = enabled;
        self
    }

    /// Include metadata in output.
    pub fn with_metadata(mut self, enabled: bool) -> Self {
        self.include_metadata = enabled;
        self
    }
}

/// JSON representation of a metric for serialization.
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonMetric {
    /// Metric name.
    pub name: String,
    /// Metric type.
    #[serde(rename = "type")]
    pub metric_type: String,
    /// Metric value (varies by type).
    pub value: serde_json::Value,
    /// Labels/tags.
    pub labels: HashMap<String, String>,
    /// Timestamp in ISO 8601 format.
    pub timestamp: String,
    /// Unix timestamp in milliseconds.
    pub timestamp_ms: i64,
    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional unit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

impl From<&Metric> for JsonMetric {
    fn from(metric: &Metric) -> Self {
        let (metric_type, value) = match &metric.value {
            MetricValue::Counter(v) => ("counter".to_string(), serde_json::json!(*v)),
            MetricValue::Gauge(v) => ("gauge".to_string(), serde_json::json!(*v)),
            MetricValue::Histogram(h) => (
                "histogram".to_string(),
                serde_json::json!({
                    "buckets": h.buckets,
                    "bucket_counts": h.bucket_counts,
                    "sum": h.sum,
                    "count": h.count,
                    "min": h.min,
                    "max": h.max,
                }),
            ),
            MetricValue::Summary(s) => (
                "summary".to_string(),
                serde_json::json!({
                    "quantiles": s.quantiles,
                    "sum": s.sum,
                    "count": s.count,
                }),
            ),
            MetricValue::Distribution(d) => (
                "distribution".to_string(),
                serde_json::json!({
                    "mean": d.mean,
                    "std_dev": d.std_dev,
                    "min": d.min,
                    "max": d.max,
                    "count": d.values.len(),
                }),
            ),
        };

        Self {
            name: metric.name.clone(),
            metric_type,
            value,
            labels: metric.labels.clone(),
            timestamp: metric.timestamp.to_rfc3339(),
            timestamp_ms: metric.timestamp.timestamp_millis(),
            description: metric.description.clone(),
            unit: metric.unit.map(|u| format!("{:?}", u)),
        }
    }
}

/// Batch of metrics with metadata.
#[derive(Debug, Serialize, Deserialize)]
pub struct MetricBatch {
    /// Batch metadata.
    pub metadata: BatchMetadata,
    /// Metrics in this batch.
    pub metrics: Vec<JsonMetric>,
}

/// Metadata about a metric batch.
#[derive(Debug, Serialize, Deserialize)]
pub struct BatchMetadata {
    /// Batch ID.
    pub batch_id: String,
    /// Export timestamp.
    pub export_time: String,
    /// Number of metrics in batch.
    pub metric_count: usize,
    /// Source host.
    pub host: String,
    /// Exporter version.
    pub exporter_version: String,
}

/// JSON file exporter for writing metrics to local files.
pub struct JsonFileExporter {
    config: JsonExporterConfig,
    current_file: Mutex<Option<CurrentFile>>,
    stats: Mutex<ExporterStats>,
}

#[allow(dead_code)]
struct CurrentFile {
    path: PathBuf,
    writer: BufWriter<File>,
    size: u64,
    metric_count: u64,
}

/// Statistics about the JSON exporter.
#[derive(Debug, Default, Clone)]
pub struct ExporterStats {
    /// Total metrics exported.
    pub total_metrics: u64,
    /// Total bytes written.
    pub total_bytes: u64,
    /// Number of file rotations.
    pub rotations: u64,
    /// Last export time.
    pub last_export: Option<DateTime<Utc>>,
    /// Number of export errors.
    pub errors: u64,
}

impl JsonFileExporter {
    /// Create a new JSON file exporter with default configuration.
    pub fn new(output_dir: impl Into<PathBuf>) -> Result<Self> {
        Self::with_config(JsonExporterConfig::new(output_dir))
    }

    /// Create a new JSON file exporter with custom configuration.
    pub fn with_config(config: JsonExporterConfig) -> Result<Self> {
        // Ensure output directory exists
        fs::create_dir_all(&config.output_dir)?;

        Ok(Self {
            config,
            current_file: Mutex::new(None),
            stats: Mutex::new(ExporterStats::default()),
        })
    }

    /// Get the current statistics.
    pub fn stats(&self) -> ExporterStats {
        self.stats.lock().clone()
    }

    /// Get the configuration.
    pub fn config(&self) -> &JsonExporterConfig {
        &self.config
    }

    /// Force rotation of the current file.
    pub fn rotate(&self) -> Result<()> {
        self.rotate_file()
    }

    fn get_current_file(&self) -> Result<PathBuf> {
        let filename = format!("{}.{}", self.config.file_prefix, self.config.file_extension);
        Ok(self.config.output_dir.join(filename))
    }

    fn get_rotated_filename(&self, index: usize) -> PathBuf {
        let extension = if self.config.compress_rotated {
            format!("{}.gz", self.config.file_extension)
        } else {
            self.config.file_extension.clone()
        };
        let filename = format!("{}.{}.{}", self.config.file_prefix, index, extension);
        self.config.output_dir.join(filename)
    }

    fn rotate_file(&self) -> Result<()> {
        let mut current = self.current_file.lock();

        // Flush and close current file
        if let Some(ref mut file) = *current {
            file.writer.flush()?;
        }
        *current = None;

        // Rotate existing files
        for i in (0..self.config.max_files).rev() {
            let current_path = if i == 0 {
                self.get_current_file()?
            } else {
                self.get_rotated_filename(i)
            };

            if current_path.exists() {
                if i + 1 >= self.config.max_files {
                    // Delete oldest file
                    fs::remove_file(&current_path)?;
                } else {
                    // Rename to next index
                    let new_path = self.get_rotated_filename(i + 1);
                    fs::rename(&current_path, &new_path)?;
                }
            }
        }

        self.stats.lock().rotations += 1;
        Ok(())
    }

    fn ensure_file(&self) -> Result<()> {
        let mut current = self.current_file.lock();

        if current.is_none() {
            let path = self.get_current_file()?;
            let file = OpenOptions::new().create(true).append(true).open(&path)?;
            let size = file.metadata().map(|m| m.len()).unwrap_or(0);

            *current = Some(CurrentFile {
                path,
                writer: BufWriter::new(file),
                size,
                metric_count: 0,
            });
        }

        // Check if rotation is needed
        if let Some(ref file) = *current
            && file.size >= self.config.max_file_size
        {
            drop(current);
            self.rotate_file()?;
            return self.ensure_file();
        }

        Ok(())
    }

    fn write_metrics(&self, metrics: &[Metric]) -> Result<usize> {
        self.ensure_file()?;

        let mut current = self.current_file.lock();
        let file = current.as_mut().ok_or_else(|| {
            ObservabilityError::MetricsExportFailed("No file available".to_string())
        })?;

        let mut bytes_written = 0;

        if self.config.json_lines_format {
            // Write each metric as a single JSON line
            for metric in metrics {
                let json_metric = JsonMetric::from(metric);
                let line = if self.config.pretty_print {
                    serde_json::to_string_pretty(&json_metric)?
                } else {
                    serde_json::to_string(&json_metric)?
                };

                writeln!(file.writer, "{}", line)?;
                bytes_written += line.len() + 1;
            }
        } else {
            // Write as a batch with metadata
            let batch = MetricBatch {
                metadata: BatchMetadata {
                    batch_id: uuid::Uuid::new_v4().to_string(),
                    export_time: Utc::now().to_rfc3339(),
                    metric_count: metrics.len(),
                    host: hostname::get()
                        .map(|h| h.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "unknown".to_string()),
                    exporter_version: env!("CARGO_PKG_VERSION").to_string(),
                },
                metrics: metrics.iter().map(JsonMetric::from).collect(),
            };

            let json = if self.config.pretty_print {
                serde_json::to_string_pretty(&batch)?
            } else {
                serde_json::to_string(&batch)?
            };

            writeln!(file.writer, "{}", json)?;
            bytes_written += json.len() + 1;
        }

        file.size += bytes_written as u64;
        file.metric_count += metrics.len() as u64;

        Ok(bytes_written)
    }
}

impl MetricExporter for JsonFileExporter {
    fn export(&self, metrics: &[Metric]) -> Result<()> {
        if metrics.is_empty() {
            return Ok(());
        }

        match self.write_metrics(metrics) {
            Ok(bytes) => {
                let mut stats = self.stats.lock();
                stats.total_metrics += metrics.len() as u64;
                stats.total_bytes += bytes as u64;
                stats.last_export = Some(Utc::now());
                Ok(())
            }
            Err(e) => {
                self.stats.lock().errors += 1;
                Err(e)
            }
        }
    }

    fn flush(&self) -> Result<()> {
        let mut current = self.current_file.lock();
        if let Some(ref mut file) = *current {
            file.writer.flush()?;
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "json-file"
    }

    fn is_healthy(&self) -> bool {
        self.config.output_dir.exists()
    }
}

/// Async-capable JSON exporter using tokio.
pub struct AsyncJsonFileExporter {
    config: JsonExporterConfig,
    stats: std::sync::Arc<parking_lot::Mutex<ExporterStats>>,
}

impl AsyncJsonFileExporter {
    /// Create a new async JSON file exporter.
    pub async fn new(output_dir: impl Into<PathBuf>) -> Result<Self> {
        let config = JsonExporterConfig::new(output_dir);
        tokio::fs::create_dir_all(&config.output_dir).await?;

        Ok(Self {
            config,
            stats: std::sync::Arc::new(parking_lot::Mutex::new(ExporterStats::default())),
        })
    }

    /// Create with custom configuration.
    pub async fn with_config(config: JsonExporterConfig) -> Result<Self> {
        tokio::fs::create_dir_all(&config.output_dir).await?;

        Ok(Self {
            config,
            stats: std::sync::Arc::new(parking_lot::Mutex::new(ExporterStats::default())),
        })
    }

    /// Export metrics asynchronously.
    pub async fn export_async(&self, metrics: &[Metric]) -> Result<()> {
        if metrics.is_empty() {
            return Ok(());
        }

        let filename = format!("{}.{}", self.config.file_prefix, self.config.file_extension);
        let path = self.config.output_dir.join(filename);

        let content = if self.config.json_lines_format {
            metrics
                .iter()
                .map(|m| {
                    let json_metric = JsonMetric::from(m);
                    serde_json::to_string(&json_metric)
                })
                .collect::<std::result::Result<Vec<_>, _>>()?
                .join("\n")
                + "\n"
        } else {
            let batch = MetricBatch {
                metadata: BatchMetadata {
                    batch_id: uuid::Uuid::new_v4().to_string(),
                    export_time: Utc::now().to_rfc3339(),
                    metric_count: metrics.len(),
                    host: hostname::get()
                        .map(|h| h.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "unknown".to_string()),
                    exporter_version: env!("CARGO_PKG_VERSION").to_string(),
                },
                metrics: metrics.iter().map(JsonMetric::from).collect(),
            };

            if self.config.pretty_print {
                serde_json::to_string_pretty(&batch)?
            } else {
                serde_json::to_string(&batch)?
            }
        };

        use tokio::io::AsyncWriteExt;

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;

        file.write_all(content.as_bytes()).await?;
        file.flush().await?;

        let mut stats = self.stats.lock();
        stats.total_metrics += metrics.len() as u64;
        stats.total_bytes += content.len() as u64;
        stats.last_export = Some(Utc::now());

        Ok(())
    }

    /// Get statistics.
    pub fn stats(&self) -> ExporterStats {
        self.stats.lock().clone()
    }
}

/// Reader for JSON metric files.
pub struct JsonMetricReader {
    path: PathBuf,
}

impl JsonMetricReader {
    /// Create a reader for the specified file.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Read all metrics from the file.
    pub fn read_all(&self) -> Result<Vec<JsonMetric>> {
        let content = fs::read_to_string(&self.path)?;
        let mut metrics = Vec::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }

            // Try parsing as single metric first
            if let Ok(metric) = serde_json::from_str::<JsonMetric>(line) {
                metrics.push(metric);
            } else if let Ok(batch) = serde_json::from_str::<MetricBatch>(line) {
                metrics.extend(batch.metrics);
            }
        }

        Ok(metrics)
    }

    /// Read metrics within a time range.
    pub fn read_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<JsonMetric>> {
        let all_metrics = self.read_all()?;

        let filtered: Vec<JsonMetric> = all_metrics
            .into_iter()
            .filter(|m| {
                if let Ok(ts) = DateTime::parse_from_rfc3339(&m.timestamp) {
                    let ts_utc = ts.with_timezone(&Utc);
                    ts_utc >= start && ts_utc <= end
                } else {
                    false
                }
            })
            .collect();

        Ok(filtered)
    }

    /// Read metrics by name pattern.
    pub fn read_by_name(&self, pattern: &str) -> Result<Vec<JsonMetric>> {
        let all_metrics = self.read_all()?;

        let filtered: Vec<JsonMetric> = all_metrics
            .into_iter()
            .filter(|m| m.name.contains(pattern))
            .collect();

        Ok(filtered)
    }

    /// Check if the file exists.
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Get the file path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_json_metric_conversion() {
        let metric = Metric::counter("test_counter", 42)
            .with_label("host", "localhost")
            .with_description("A test counter");

        let json_metric = JsonMetric::from(&metric);

        assert_eq!(json_metric.name, "test_counter");
        assert_eq!(json_metric.metric_type, "counter");
        assert_eq!(json_metric.value, serde_json::json!(42));
        assert_eq!(
            json_metric.labels.get("host"),
            Some(&"localhost".to_string())
        );
    }

    #[test]
    fn test_json_file_exporter() {
        let temp_dir = env::temp_dir().join("oxigdal-test-json-exporter");
        let _ = fs::remove_dir_all(&temp_dir);

        let config = JsonExporterConfig::new(&temp_dir)
            .with_prefix("test_metrics")
            .with_json_lines(true);

        let exporter = JsonFileExporter::with_config(config).expect("should create exporter");

        let metrics = vec![
            Metric::counter("requests", 100).with_label("method", "GET"),
            Metric::gauge("temperature", 23.5),
        ];

        exporter.export(&metrics).expect("export should succeed");
        exporter.flush().expect("flush should succeed");

        let stats = exporter.stats();
        assert_eq!(stats.total_metrics, 2);
        assert!(stats.total_bytes > 0);

        // Clean up
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_json_metric_reader() {
        let temp_dir = env::temp_dir().join("oxigdal-test-json-reader");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).expect("should create dir");

        // Write some test data
        let test_file = temp_dir.join("test_metrics.json");
        let metrics = [
            JsonMetric {
                name: "metric1".to_string(),
                metric_type: "counter".to_string(),
                value: serde_json::json!(100),
                labels: HashMap::new(),
                timestamp: Utc::now().to_rfc3339(),
                timestamp_ms: Utc::now().timestamp_millis(),
                description: None,
                unit: None,
            },
            JsonMetric {
                name: "metric2".to_string(),
                metric_type: "gauge".to_string(),
                value: serde_json::json!(50.5),
                labels: HashMap::new(),
                timestamp: Utc::now().to_rfc3339(),
                timestamp_ms: Utc::now().timestamp_millis(),
                description: None,
                unit: None,
            },
        ];

        let content: String = metrics
            .iter()
            .map(|m| serde_json::to_string(m).expect("serialize"))
            .collect::<Vec<_>>()
            .join("\n");

        fs::write(&test_file, content).expect("write file");

        let reader = JsonMetricReader::new(&test_file);
        assert!(reader.exists());

        let read_metrics = reader.read_all().expect("should read metrics");
        assert_eq!(read_metrics.len(), 2);

        let by_name = reader.read_by_name("metric1").expect("should filter");
        assert_eq!(by_name.len(), 1);

        // Clean up
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_config_builder() {
        let config = JsonExporterConfig::new("/tmp/metrics")
            .with_prefix("app_metrics")
            .with_pretty_print(true)
            .with_max_file_size(50 * 1024 * 1024)
            .with_max_files(5)
            .with_json_lines(false)
            .with_metadata(true);

        assert_eq!(config.output_dir, PathBuf::from("/tmp/metrics"));
        assert_eq!(config.file_prefix, "app_metrics");
        assert!(config.pretty_print);
        assert_eq!(config.max_file_size, 50 * 1024 * 1024);
        assert_eq!(config.max_files, 5);
        assert!(!config.json_lines_format);
        assert!(config.include_metadata);
    }
}

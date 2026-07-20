//! Metric types and definitions for Kinesis streams

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metric type for Kinesis streams
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricType {
    /// Incoming records
    IncomingRecords,
    /// Incoming bytes
    IncomingBytes,
    /// Outgoing records
    OutgoingRecords,
    /// Outgoing bytes
    OutgoingBytes,
    /// Write throughput exceeded
    WriteProvisionedThroughputExceeded,
    /// Read throughput exceeded
    ReadProvisionedThroughputExceeded,
    /// Iterator age (milliseconds)
    GetRecordsIteratorAgeMilliseconds,
    /// Get records latency
    GetRecordsLatency,
    /// Put record latency
    PutRecordLatency,
    /// Put records latency
    PutRecordsLatency,
    /// Put record success
    PutRecordSuccess,
    /// Put records success
    PutRecordsSuccess,
    /// Put records failed records
    PutRecordsFailedRecords,
    /// Put records successful records
    PutRecordsSuccessfulRecords,
    /// Put records throttled records
    PutRecordsThrottledRecords,
    /// Put records total records
    PutRecordsTotalRecords,
    /// Subscription iterator age
    SubscribeToShardIteratorAgeMilliseconds,
}

impl MetricType {
    /// Converts to CloudWatch metric name
    pub fn as_str(&self) -> &str {
        match self {
            Self::IncomingRecords => "IncomingRecords",
            Self::IncomingBytes => "IncomingBytes",
            Self::OutgoingRecords => "OutgoingRecords",
            Self::OutgoingBytes => "OutgoingBytes",
            Self::WriteProvisionedThroughputExceeded => "WriteProvisionedThroughputExceeded",
            Self::ReadProvisionedThroughputExceeded => "ReadProvisionedThroughputExceeded",
            Self::GetRecordsIteratorAgeMilliseconds => "GetRecords.IteratorAgeMilliseconds",
            Self::GetRecordsLatency => "GetRecords.Latency",
            Self::PutRecordLatency => "PutRecord.Latency",
            Self::PutRecordsLatency => "PutRecords.Latency",
            Self::PutRecordSuccess => "PutRecord.Success",
            Self::PutRecordsSuccess => "PutRecords.Success",
            Self::PutRecordsFailedRecords => "PutRecords.FailedRecords",
            Self::PutRecordsSuccessfulRecords => "PutRecords.SuccessfulRecords",
            Self::PutRecordsThrottledRecords => "PutRecords.ThrottledRecords",
            Self::PutRecordsTotalRecords => "PutRecords.TotalRecords",
            Self::SubscribeToShardIteratorAgeMilliseconds => {
                "SubscribeToShard.IteratorAgeMilliseconds"
            }
        }
    }

    /// Gets the unit for this metric
    pub fn unit(&self) -> &str {
        match self {
            Self::IncomingRecords
            | Self::OutgoingRecords
            | Self::PutRecordsFailedRecords
            | Self::PutRecordsSuccessfulRecords
            | Self::PutRecordsThrottledRecords
            | Self::PutRecordsTotalRecords => "Count",
            Self::IncomingBytes | Self::OutgoingBytes => "Bytes",
            Self::GetRecordsIteratorAgeMilliseconds
            | Self::SubscribeToShardIteratorAgeMilliseconds => "Milliseconds",
            Self::GetRecordsLatency | Self::PutRecordLatency | Self::PutRecordsLatency => {
                "Milliseconds"
            }
            Self::WriteProvisionedThroughputExceeded | Self::ReadProvisionedThroughputExceeded => {
                "Count"
            }
            Self::PutRecordSuccess | Self::PutRecordsSuccess => "Count",
        }
    }

    /// Checks if this is a latency metric
    pub fn is_latency(&self) -> bool {
        matches!(
            self,
            Self::GetRecordsLatency | Self::PutRecordLatency | Self::PutRecordsLatency
        )
    }

    /// Checks if this is a throughput metric
    pub fn is_throughput(&self) -> bool {
        matches!(
            self,
            Self::IncomingRecords
                | Self::IncomingBytes
                | Self::OutgoingRecords
                | Self::OutgoingBytes
        )
    }

    /// Checks if this is an error metric
    pub fn is_error(&self) -> bool {
        matches!(
            self,
            Self::WriteProvisionedThroughputExceeded
                | Self::ReadProvisionedThroughputExceeded
                | Self::PutRecordsFailedRecords
                | Self::PutRecordsThrottledRecords
        )
    }
}

/// Stream metrics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamMetrics {
    /// Stream name
    pub stream_name: String,
    /// Metrics values
    pub metrics: HashMap<MetricType, f64>,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl StreamMetrics {
    /// Creates a new stream metrics snapshot
    pub fn new(stream_name: impl Into<String>) -> Self {
        Self {
            stream_name: stream_name.into(),
            metrics: HashMap::new(),
            timestamp: chrono::Utc::now(),
        }
    }

    /// Sets a metric value
    pub fn set_metric(&mut self, metric_type: MetricType, value: f64) {
        self.metrics.insert(metric_type, value);
    }

    /// Gets a metric value
    pub fn get_metric(&self, metric_type: MetricType) -> Option<f64> {
        self.metrics.get(&metric_type).copied()
    }

    /// Gets incoming records per second
    pub fn incoming_records_per_second(&self) -> Option<f64> {
        self.get_metric(MetricType::IncomingRecords)
    }

    /// Gets incoming bytes per second
    pub fn incoming_bytes_per_second(&self) -> Option<f64> {
        self.get_metric(MetricType::IncomingBytes)
    }

    /// Gets iterator age in milliseconds
    pub fn iterator_age_ms(&self) -> Option<f64> {
        self.get_metric(MetricType::GetRecordsIteratorAgeMilliseconds)
    }

    /// Checks if the stream has high iterator age (> 60 seconds)
    pub fn has_high_iterator_age(&self) -> bool {
        self.iterator_age_ms()
            .map(|age| age > 60_000.0)
            .unwrap_or(false)
    }

    /// Checks if the stream is experiencing throttling
    pub fn is_throttled(&self) -> bool {
        self.get_metric(MetricType::WriteProvisionedThroughputExceeded)
            .map(|count| count > 0.0)
            .unwrap_or(false)
            || self
                .get_metric(MetricType::ReadProvisionedThroughputExceeded)
                .map(|count| count > 0.0)
                .unwrap_or(false)
    }
}

/// Shard metrics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardMetrics {
    /// Stream name
    pub stream_name: String,
    /// Shard ID
    pub shard_id: String,
    /// Metrics values
    pub metrics: HashMap<MetricType, f64>,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl ShardMetrics {
    /// Creates a new shard metrics snapshot
    pub fn new(stream_name: impl Into<String>, shard_id: impl Into<String>) -> Self {
        Self {
            stream_name: stream_name.into(),
            shard_id: shard_id.into(),
            metrics: HashMap::new(),
            timestamp: chrono::Utc::now(),
        }
    }

    /// Sets a metric value
    pub fn set_metric(&mut self, metric_type: MetricType, value: f64) {
        self.metrics.insert(metric_type, value);
    }

    /// Gets a metric value
    pub fn get_metric(&self, metric_type: MetricType) -> Option<f64> {
        self.metrics.get(&metric_type).copied()
    }

    /// Gets incoming records per second
    pub fn incoming_records_per_second(&self) -> Option<f64> {
        self.get_metric(MetricType::IncomingRecords)
    }

    /// Gets outgoing records per second
    pub fn outgoing_records_per_second(&self) -> Option<f64> {
        self.get_metric(MetricType::OutgoingRecords)
    }
}

/// Metrics aggregator
#[derive(Default)]
pub struct MetricsAggregator {
    stream_metrics: parking_lot::RwLock<HashMap<String, StreamMetrics>>,
    shard_metrics: parking_lot::RwLock<HashMap<(String, String), ShardMetrics>>,
}

impl MetricsAggregator {
    /// Creates a new metrics aggregator
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates stream metrics
    pub fn update_stream_metrics(&self, metrics: StreamMetrics) {
        self.stream_metrics
            .write()
            .insert(metrics.stream_name.clone(), metrics);
    }

    /// Gets stream metrics
    pub fn get_stream_metrics(&self, stream_name: &str) -> Option<StreamMetrics> {
        self.stream_metrics.read().get(stream_name).cloned()
    }

    /// Updates shard metrics
    pub fn update_shard_metrics(&self, metrics: ShardMetrics) {
        self.shard_metrics.write().insert(
            (metrics.stream_name.clone(), metrics.shard_id.clone()),
            metrics,
        );
    }

    /// Gets shard metrics
    pub fn get_shard_metrics(&self, stream_name: &str, shard_id: &str) -> Option<ShardMetrics> {
        self.shard_metrics
            .read()
            .get(&(stream_name.to_string(), shard_id.to_string()))
            .cloned()
    }

    /// Lists all stream names
    pub fn list_streams(&self) -> Vec<String> {
        self.stream_metrics.read().keys().cloned().collect()
    }

    /// Lists all shard IDs for a stream
    pub fn list_shards(&self, stream_name: &str) -> Vec<String> {
        self.shard_metrics
            .read()
            .keys()
            .filter(|(s, _)| s == stream_name)
            .map(|(_, shard_id)| shard_id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_type_conversion() {
        assert_eq!(MetricType::IncomingRecords.as_str(), "IncomingRecords");
        assert_eq!(MetricType::IncomingBytes.as_str(), "IncomingBytes");
        assert_eq!(
            MetricType::GetRecordsIteratorAgeMilliseconds.as_str(),
            "GetRecords.IteratorAgeMilliseconds"
        );
    }

    #[test]
    fn test_metric_type_unit() {
        assert_eq!(MetricType::IncomingRecords.unit(), "Count");
        assert_eq!(MetricType::IncomingBytes.unit(), "Bytes");
        assert_eq!(MetricType::GetRecordsLatency.unit(), "Milliseconds");
    }

    #[test]
    fn test_metric_type_categories() {
        assert!(MetricType::IncomingRecords.is_throughput());
        assert!(MetricType::GetRecordsLatency.is_latency());
        assert!(MetricType::WriteProvisionedThroughputExceeded.is_error());
    }

    #[test]
    fn test_stream_metrics() {
        let mut metrics = StreamMetrics::new("test-stream");
        metrics.set_metric(MetricType::IncomingRecords, 1000.0);
        metrics.set_metric(MetricType::GetRecordsIteratorAgeMilliseconds, 5000.0);

        assert_eq!(metrics.incoming_records_per_second(), Some(1000.0));
        assert_eq!(metrics.iterator_age_ms(), Some(5000.0));
        assert!(!metrics.has_high_iterator_age());
    }

    #[test]
    fn test_stream_metrics_high_iterator_age() {
        let mut metrics = StreamMetrics::new("test-stream");
        metrics.set_metric(MetricType::GetRecordsIteratorAgeMilliseconds, 120_000.0);

        assert!(metrics.has_high_iterator_age());
    }

    #[test]
    fn test_stream_metrics_throttling() {
        let mut metrics = StreamMetrics::new("test-stream");
        metrics.set_metric(MetricType::WriteProvisionedThroughputExceeded, 10.0);

        assert!(metrics.is_throttled());
    }

    #[test]
    fn test_shard_metrics() {
        let mut metrics = ShardMetrics::new("test-stream", "shard-0001");
        metrics.set_metric(MetricType::IncomingRecords, 500.0);
        metrics.set_metric(MetricType::OutgoingRecords, 450.0);

        assert_eq!(metrics.incoming_records_per_second(), Some(500.0));
        assert_eq!(metrics.outgoing_records_per_second(), Some(450.0));
    }

    #[test]
    fn test_metrics_aggregator() {
        let aggregator = MetricsAggregator::new();

        let mut stream_metrics = StreamMetrics::new("test-stream");
        stream_metrics.set_metric(MetricType::IncomingRecords, 1000.0);
        aggregator.update_stream_metrics(stream_metrics);

        let retrieved = aggregator.get_stream_metrics("test-stream");
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.and_then(|m| m.get_metric(MetricType::IncomingRecords)),
            Some(1000.0)
        );
    }

    #[test]
    fn test_metrics_aggregator_shard() {
        let aggregator = MetricsAggregator::new();

        let mut shard_metrics = ShardMetrics::new("test-stream", "shard-0001");
        shard_metrics.set_metric(MetricType::IncomingRecords, 500.0);
        aggregator.update_shard_metrics(shard_metrics);

        let retrieved = aggregator.get_shard_metrics("test-stream", "shard-0001");
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_metrics_aggregator_list() {
        let aggregator = MetricsAggregator::new();

        aggregator.update_stream_metrics(StreamMetrics::new("stream-1"));
        aggregator.update_stream_metrics(StreamMetrics::new("stream-2"));

        let streams = aggregator.list_streams();
        assert_eq!(streams.len(), 2);
    }
}

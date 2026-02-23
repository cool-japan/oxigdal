//! Monitoring and metrics for Kinesis streams

pub mod alerts;
pub mod cloudwatch;
pub mod metrics;

pub use alerts::{Alert, AlertAction, AlertCondition};
pub use cloudwatch::{CloudWatchMonitor, MetricFilter, MetricStatisticsParams};
pub use metrics::{MetricType, ShardMetrics, StreamMetrics};

use crate::error::Result;
use aws_sdk_cloudwatch::Client as CloudWatchClient;
use std::sync::Arc;

/// Kinesis monitoring client
#[derive(Clone)]
pub struct KinesisMonitoring {
    client: Arc<CloudWatchClient>,
    namespace: String,
}

impl KinesisMonitoring {
    /// Creates a new Kinesis monitoring client
    pub fn new(client: CloudWatchClient) -> Self {
        Self {
            client: Arc::new(client),
            namespace: "AWS/Kinesis".to_string(),
        }
    }

    /// Creates a new Kinesis monitoring client from environment
    pub async fn from_env() -> Self {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = CloudWatchClient::new(&config);
        Self::new(client)
    }

    /// Gets a reference to the CloudWatch client
    pub fn client(&self) -> &CloudWatchClient {
        &self.client
    }

    /// Gets the namespace
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Gets stream metrics
    pub async fn get_stream_metrics(
        &self,
        stream_name: &str,
        metric_type: MetricType,
        start_time: chrono::DateTime<chrono::Utc>,
        end_time: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<f64>> {
        let monitor = CloudWatchMonitor::new(self.client.as_ref().clone());
        let params = MetricStatisticsParams {
            namespace: &self.namespace,
            metric_name: metric_type.as_str(),
            dimensions: vec![("StreamName", stream_name)],
            start_time,
            end_time,
            period_seconds: 300, // 5 minutes
            statistics: vec!["Average"],
        };
        monitor.get_metric_statistics(params).await
    }

    /// Gets shard-level metrics
    pub async fn get_shard_metrics(
        &self,
        stream_name: &str,
        shard_id: &str,
        metric_type: MetricType,
        start_time: chrono::DateTime<chrono::Utc>,
        end_time: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<f64>> {
        let monitor = CloudWatchMonitor::new(self.client.as_ref().clone());
        let params = MetricStatisticsParams {
            namespace: &self.namespace,
            metric_name: metric_type.as_str(),
            dimensions: vec![("StreamName", stream_name), ("ShardId", shard_id)],
            start_time,
            end_time,
            period_seconds: 60, // 1 minute
            statistics: vec!["Average"],
        };
        monitor.get_metric_statistics(params).await
    }

    /// Puts a custom metric
    pub async fn put_metric(
        &self,
        metric_name: &str,
        value: f64,
        unit: &str,
        dimensions: Vec<(&str, &str)>,
    ) -> Result<()> {
        let monitor = CloudWatchMonitor::new(self.client.as_ref().clone());
        monitor
            .put_metric(&self.namespace, metric_name, value, unit, dimensions)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_type_conversion() {
        assert_eq!(MetricType::IncomingRecords.as_str(), "IncomingRecords");
        assert_eq!(MetricType::IncomingBytes.as_str(), "IncomingBytes");
        assert_eq!(MetricType::OutgoingRecords.as_str(), "OutgoingRecords");
    }
}

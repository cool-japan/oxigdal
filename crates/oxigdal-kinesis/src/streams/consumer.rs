//! Kinesis consumer with enhanced fan-out support

use crate::error::{KinesisError, Result};
use crate::streams::shard::ShardIteratorType;
use aws_sdk_kinesis::Client as KinesisClient;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::{Duration, sleep};
#[cfg(feature = "enhanced-fanout")]
use tracing::{debug, info};

/// Consumer configuration
#[derive(Debug, Clone)]
pub struct ConsumerConfig {
    /// Stream name
    pub stream_name: String,
    /// Consumer name (for enhanced fan-out)
    pub consumer_name: Option<String>,
    /// Shard iterator type
    pub iterator_type: ShardIteratorType,
    /// Maximum records per batch
    pub max_records: i32,
    /// Polling interval in milliseconds (for standard consumers)
    pub poll_interval_ms: u64,
    /// Enable enhanced fan-out
    pub enhanced_fanout: bool,
    /// Retry attempts
    pub retry_attempts: u32,
    /// Retry backoff base in milliseconds
    pub retry_backoff_ms: u64,
}

impl Default for ConsumerConfig {
    fn default() -> Self {
        Self {
            stream_name: String::new(),
            consumer_name: None,
            iterator_type: ShardIteratorType::Latest,
            max_records: 10000,
            poll_interval_ms: 1000,
            enhanced_fanout: false,
            retry_attempts: 3,
            retry_backoff_ms: 100,
        }
    }
}

impl ConsumerConfig {
    /// Creates a new consumer configuration
    pub fn new(stream_name: impl Into<String>) -> Self {
        Self {
            stream_name: stream_name.into(),
            ..Default::default()
        }
    }

    /// Sets the consumer name
    pub fn with_consumer_name(mut self, name: impl Into<String>) -> Self {
        self.consumer_name = Some(name.into());
        self
    }

    /// Sets the shard iterator type
    pub fn with_iterator_type(mut self, iterator_type: ShardIteratorType) -> Self {
        self.iterator_type = iterator_type;
        self
    }

    /// Sets the maximum records per batch
    pub fn with_max_records(mut self, max_records: i32) -> Self {
        self.max_records = max_records;
        self
    }

    /// Sets the poll interval
    pub fn with_poll_interval_ms(mut self, ms: u64) -> Self {
        self.poll_interval_ms = ms;
        self
    }

    /// Enables enhanced fan-out
    pub fn with_enhanced_fanout(mut self, enabled: bool) -> Self {
        self.enhanced_fanout = enabled;
        self
    }

    /// Sets retry attempts
    pub fn with_retry_attempts(mut self, attempts: u32) -> Self {
        self.retry_attempts = attempts;
        self
    }
}

/// Record received from Kinesis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumerRecord {
    /// Sequence number
    pub sequence_number: String,
    /// Partition key
    pub partition_key: String,
    /// Data payload
    pub data: Bytes,
    /// Approximate arrival timestamp
    pub approximate_arrival_timestamp: Option<i64>,
    /// Encryption type
    pub encryption_type: Option<String>,
}

impl ConsumerRecord {
    /// Creates a new consumer record
    pub fn new(
        sequence_number: impl Into<String>,
        partition_key: impl Into<String>,
        data: Bytes,
    ) -> Self {
        Self {
            sequence_number: sequence_number.into(),
            partition_key: partition_key.into(),
            data,
            approximate_arrival_timestamp: None,
            encryption_type: None,
        }
    }

    /// Sets the arrival timestamp
    pub fn with_arrival_timestamp(mut self, timestamp: i64) -> Self {
        self.approximate_arrival_timestamp = Some(timestamp);
        self
    }

    /// Sets the encryption type
    pub fn with_encryption_type(mut self, encryption_type: impl Into<String>) -> Self {
        self.encryption_type = Some(encryption_type.into());
        self
    }
}

/// Standard Kinesis consumer (polling-based)
pub struct Consumer {
    client: Arc<KinesisClient>,
    config: ConsumerConfig,
    shard_id: String,
    shard_iterator: Option<String>,
    metrics: Arc<ConsumerMetrics>,
}

impl Consumer {
    /// Creates a new consumer for a specific shard
    pub async fn new(
        client: KinesisClient,
        config: ConsumerConfig,
        shard_id: impl Into<String>,
    ) -> Result<Self> {
        let shard_id = shard_id.into();
        let metrics = Arc::new(ConsumerMetrics::default());

        let mut consumer = Self {
            client: Arc::new(client),
            config,
            shard_id,
            shard_iterator: None,
            metrics,
        };

        // Initialize shard iterator
        consumer.initialize_iterator().await?;

        Ok(consumer)
    }

    /// Initializes the shard iterator
    async fn initialize_iterator(&mut self) -> Result<()> {
        let iterator_type = match &self.config.iterator_type {
            ShardIteratorType::TrimHorizon => "TRIM_HORIZON",
            ShardIteratorType::Latest => "LATEST",
            ShardIteratorType::AtSequenceNumber(_) => "AT_SEQUENCE_NUMBER",
            ShardIteratorType::AfterSequenceNumber(_) => "AFTER_SEQUENCE_NUMBER",
            ShardIteratorType::AtTimestamp(_) => "AT_TIMESTAMP",
        };

        let mut request = self
            .client
            .get_shard_iterator()
            .stream_name(&self.config.stream_name)
            .shard_id(&self.shard_id)
            .shard_iterator_type(aws_sdk_kinesis::types::ShardIteratorType::from(
                iterator_type,
            ));

        // Set sequence number if applicable
        if let ShardIteratorType::AtSequenceNumber(seq)
        | ShardIteratorType::AfterSequenceNumber(seq) = &self.config.iterator_type
        {
            request = request.starting_sequence_number(seq);
        }

        // Set timestamp if applicable
        if let ShardIteratorType::AtTimestamp(ts) = &self.config.iterator_type {
            request = request.timestamp(aws_sdk_kinesis::primitives::DateTime::from_secs(*ts));
        }

        let response = request.send().await.map_err(|e| KinesisError::Service {
            message: e.to_string(),
        })?;

        self.shard_iterator = response.shard_iterator().map(|s| s.to_string());

        Ok(())
    }

    /// Polls for records from the stream
    pub async fn poll(&mut self) -> Result<Vec<ConsumerRecord>> {
        if self.shard_iterator.is_none() {
            return Err(KinesisError::InvalidState {
                message: "Shard iterator not initialized".to_string(),
            });
        }

        let iterator = self
            .shard_iterator
            .as_ref()
            .ok_or_else(|| KinesisError::InvalidState {
                message: "Shard iterator is None".to_string(),
            })?;

        let response = self
            .client
            .get_records()
            .shard_iterator(iterator)
            .limit(self.config.max_records)
            .send()
            .await
            .map_err(|e| KinesisError::Service {
                message: e.to_string(),
            })?;

        // Update shard iterator for next call
        self.shard_iterator = response.next_shard_iterator().map(|s| s.to_string());

        let records: Vec<ConsumerRecord> = response
            .records()
            .iter()
            .map(|record| {
                let data = Bytes::copy_from_slice(record.data().as_ref());

                let mut consumer_record =
                    ConsumerRecord::new(record.sequence_number(), record.partition_key(), data);

                if let Some(ts) = record.approximate_arrival_timestamp() {
                    consumer_record = consumer_record.with_arrival_timestamp(ts.secs());
                }

                if let Some(enc) = record.encryption_type() {
                    consumer_record = consumer_record.with_encryption_type(enc.as_str());
                }

                consumer_record
            })
            .collect();

        self.metrics
            .increment_records_received(records.len() as u64);

        // Check for millis_behind_latest
        if let Some(millis_behind) = response.millis_behind_latest() {
            self.metrics.update_millis_behind_latest(millis_behind);
        }

        Ok(records)
    }

    /// Polls continuously with callback
    pub async fn poll_loop<F>(&mut self, mut callback: F) -> Result<()>
    where
        F: FnMut(ConsumerRecord) -> Result<()>,
    {
        loop {
            let records = self.poll().await?;

            if records.is_empty() {
                // No records, sleep and continue
                sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
                continue;
            }

            for record in records {
                callback(record)?;
            }
        }
    }

    /// Gets consumer metrics
    pub fn metrics(&self) -> &ConsumerMetrics {
        &self.metrics
    }

    /// Gets the shard ID
    pub fn shard_id(&self) -> &str {
        &self.shard_id
    }
}

/// Enhanced fan-out consumer (push-based using SubscribeToShard)
#[cfg(feature = "enhanced-fanout")]
pub struct EnhancedFanOutConsumer {
    client: Arc<KinesisClient>,
    #[allow(dead_code)]
    config: ConsumerConfig,
    consumer_arn: String,
    metrics: Arc<ConsumerMetrics>,
}

#[cfg(feature = "enhanced-fanout")]
impl EnhancedFanOutConsumer {
    /// Creates a new enhanced fan-out consumer
    pub async fn new(client: KinesisClient, config: ConsumerConfig) -> Result<Self> {
        let client = Arc::new(client);
        let consumer_name =
            config
                .consumer_name
                .clone()
                .ok_or_else(|| KinesisError::InvalidConfig {
                    message: "Consumer name required for enhanced fan-out".to_string(),
                })?;

        // Register stream consumer
        let consumer_arn =
            Self::register_consumer(&client, &config.stream_name, &consumer_name).await?;

        let metrics = Arc::new(ConsumerMetrics::default());

        Ok(Self {
            client,
            config,
            consumer_arn,
            metrics,
        })
    }

    /// Registers a stream consumer
    #[allow(clippy::needless_borrow)]
    async fn register_consumer(
        client: &KinesisClient,
        stream_name: &str,
        consumer_name: &str,
    ) -> Result<String> {
        // Check if consumer already exists
        match client
            .describe_stream_consumer()
            .stream_arn(stream_name)
            .consumer_name(consumer_name)
            .send()
            .await
        {
            Ok(response) => {
                if let Some(consumer) = response.consumer_description() {
                    info!("Using existing consumer: {}", consumer_name);
                    return Ok(consumer.consumer_arn().to_string());
                }
            }
            Err(_) => {
                // Consumer doesn't exist, create it
            }
        }

        info!("Registering new consumer: {}", consumer_name);
        let response = client
            .register_stream_consumer()
            .stream_arn(stream_name)
            .consumer_name(consumer_name)
            .send()
            .await
            .map_err(|e| KinesisError::Service {
                message: e.to_string(),
            })?;

        let consumer_arn = response
            .consumer()
            .map(|c| c.consumer_arn().to_string())
            .ok_or_else(|| KinesisError::Service {
                message: "Consumer ARN not returned".to_string(),
            })?;

        // Wait for consumer to become active
        Self::wait_for_consumer_active(&client, &consumer_arn).await?;

        Ok(consumer_arn)
    }

    /// Waits for consumer to become active
    async fn wait_for_consumer_active(client: &KinesisClient, consumer_arn: &str) -> Result<()> {
        loop {
            let response = client
                .describe_stream_consumer()
                .consumer_arn(consumer_arn)
                .send()
                .await
                .map_err(|e| KinesisError::Service {
                    message: e.to_string(),
                })?;

            if let Some(consumer) = response.consumer_description() {
                let status = consumer.consumer_status();
                if status.as_str() == "ACTIVE" {
                    info!("Consumer is now active");
                    return Ok(());
                } else if status.as_str() == "CREATING" {
                    debug!("Waiting for consumer to become active...");
                    sleep(Duration::from_secs(1)).await;
                } else {
                    return Err(KinesisError::InvalidState {
                        message: format!("Consumer in unexpected state: {}", status.as_str()),
                    });
                }
            } else {
                return Err(KinesisError::InvalidState {
                    message: "Consumer description not available".to_string(),
                });
            }
        }
    }

    /// Deregisters the consumer
    pub async fn deregister(&self) -> Result<()> {
        self.client
            .deregister_stream_consumer()
            .consumer_arn(&self.consumer_arn)
            .send()
            .await
            .map_err(|e| KinesisError::Service {
                message: e.to_string(),
            })?;

        info!("Consumer deregistered: {}", self.consumer_arn);
        Ok(())
    }

    /// Gets consumer metrics
    pub fn metrics(&self) -> &ConsumerMetrics {
        &self.metrics
    }
}

/// Consumer metrics
#[derive(Default)]
pub struct ConsumerMetrics {
    records_received: parking_lot::Mutex<u64>,
    millis_behind_latest: parking_lot::Mutex<i64>,
}

impl ConsumerMetrics {
    fn increment_records_received(&self, count: u64) {
        *self.records_received.lock() += count;
    }

    fn update_millis_behind_latest(&self, millis: i64) {
        *self.millis_behind_latest.lock() = millis;
    }

    /// Gets the number of records received
    pub fn records_received(&self) -> u64 {
        *self.records_received.lock()
    }

    /// Gets milliseconds behind latest
    pub fn millis_behind_latest(&self) -> i64 {
        *self.millis_behind_latest.lock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consumer_record_creation() {
        let record = ConsumerRecord::new("seq-123", "partition-1", Bytes::from("test data"));
        assert_eq!(record.sequence_number, "seq-123");
        assert_eq!(record.partition_key, "partition-1");
        assert_eq!(record.data, Bytes::from("test data"));
    }

    #[test]
    fn test_consumer_config() {
        let config = ConsumerConfig::new("test-stream")
            .with_consumer_name("test-consumer")
            .with_max_records(5000)
            .with_enhanced_fanout(true);

        assert_eq!(config.stream_name, "test-stream");
        assert_eq!(config.consumer_name, Some("test-consumer".to_string()));
        assert_eq!(config.max_records, 5000);
        assert!(config.enhanced_fanout);
    }

    #[test]
    fn test_consumer_metrics() {
        let metrics = ConsumerMetrics::default();
        assert_eq!(metrics.records_received(), 0);
        assert_eq!(metrics.millis_behind_latest(), 0);

        metrics.increment_records_received(10);
        assert_eq!(metrics.records_received(), 10);

        metrics.update_millis_behind_latest(5000);
        assert_eq!(metrics.millis_behind_latest(), 5000);
    }
}

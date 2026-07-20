//! Kafka producer implementation

mod batch;
mod config;
mod metrics;
mod partitioner;

pub use batch::{BatchManager, MessageBatch};
pub use config::{ProducerConfig, ProducerConfigBuilder};
pub use metrics::{AtomicProducerMetrics, MessageStats, ProducerMetrics};
pub use partitioner::{CustomPartitioner, Partitioner, PartitionerType, create_partitioner};

use crate::error::{Error, Result};
use crate::utils;

use async_trait::async_trait;
use bytes::Bytes;
use rdkafka::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord, Producer};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Kafka producer for async message production
pub struct KafkaProducer {
    /// Internal rdkafka producer
    producer: Arc<FutureProducer>,
    /// Producer configuration
    config: ProducerConfig,
    /// Partitioner
    partitioner: Arc<dyn Partitioner>,
    /// Batch manager (reserved for future batching optimizations)
    #[allow(dead_code)]
    batch_manager: Option<Arc<BatchManager>>,
    /// Producer metrics
    metrics: Arc<RwLock<ProducerMetrics>>,
}

impl KafkaProducer {
    /// Create a new Kafka producer
    pub async fn new(config: ProducerConfig) -> Result<Self> {
        info!("Creating Kafka producer with config: {:?}", config);

        let mut client_config = ClientConfig::new();
        config
            .base_config
            .apply_to_client_config(&mut client_config);

        // Producer-specific configuration
        client_config.set("acks", config.acks.to_rdkafka_str());
        client_config.set("compression.type", config.compression_type.to_rdkafka_str());
        client_config.set(
            "max.in.flight.requests.per.connection",
            config.max_in_flight.to_string(),
        );
        client_config.set("linger.ms", config.linger_ms.to_string());
        client_config.set("batch.size", config.batch_size.to_string());
        client_config.set("batch.num.messages", config.batch_num_messages.to_string());
        client_config.set("request.timeout.ms", config.request_timeout_ms.to_string());
        client_config.set("message.timeout.ms", config.message_timeout_ms.to_string());
        client_config.set("enable.idempotence", config.enable_idempotence.to_string());

        if let Some(max_retries) = config.max_retries {
            client_config.set("message.send.max.retries", max_retries.to_string());
        }

        if let Some(retry_backoff_ms) = config.retry_backoff_ms {
            client_config.set("retry.backoff.ms", retry_backoff_ms.to_string());
        }

        let producer: FutureProducer = client_config
            .create()
            .map_err(|e| Error::Configuration(format!("Failed to create producer: {}", e)))?;

        let partitioner = partitioner::create_partitioner(&config.partitioner_type);

        let batch_manager = if config.enable_batching {
            Some(Arc::new(BatchManager::new(
                config.batch_size,
                config.batch_num_messages,
                Duration::from_millis(config.linger_ms as u64),
            )))
        } else {
            None
        };

        info!("Kafka producer created successfully");

        Ok(Self {
            producer: Arc::new(producer),
            config,
            partitioner,
            batch_manager,
            metrics: Arc::new(RwLock::new(ProducerMetrics::default())),
        })
    }

    /// Send a message to a topic
    pub async fn send(&self, topic: &str, key: Option<&[u8]>, value: &[u8]) -> Result<(i32, i64)> {
        utils::validate_topic_name(topic)?;

        let start = std::time::Instant::now();

        let partition = if let Some(key) = key {
            let num_partitions = self.get_partition_count(topic).await?;
            Some(self.partitioner.partition(topic, key, num_partitions))
        } else {
            None
        };

        let mut record = FutureRecord::to(topic).payload(value);

        if let Some(key) = key {
            record = record.key(key);
        }

        if let Some(partition) = partition {
            record = record.partition(partition);
        }

        let delivery_status = self
            .producer
            .send(
                record,
                Duration::from_millis(self.config.message_timeout_ms as u64),
            )
            .await
            .map_err(|(err, _)| Error::Producer(format!("Failed to send message: {}", err)))?;

        let elapsed = start.elapsed();

        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.record_message_sent(value.len(), elapsed);

        debug!(
            "Message sent to topic {} partition {} offset {} in {:?}",
            topic, delivery_status.partition, delivery_status.offset, elapsed
        );

        Ok((delivery_status.partition, delivery_status.offset))
    }

    /// Send a message with custom headers
    pub async fn send_with_headers(
        &self,
        topic: &str,
        key: Option<&[u8]>,
        value: &[u8],
        headers: Vec<(String, Vec<u8>)>,
    ) -> Result<(i32, i64)> {
        utils::validate_topic_name(topic)?;

        let start = std::time::Instant::now();

        let partition = if let Some(key) = key {
            let num_partitions = self.get_partition_count(topic).await?;
            Some(self.partitioner.partition(topic, key, num_partitions))
        } else {
            None
        };

        let mut record = FutureRecord::to(topic).payload(value);

        if let Some(key) = key {
            record = record.key(key);
        }

        if let Some(partition) = partition {
            record = record.partition(partition);
        }

        for (key, value) in headers {
            record = record.headers(rdkafka::message::OwnedHeaders::new().insert(
                rdkafka::message::Header {
                    key: &key,
                    value: Some(&value),
                },
            ));
        }

        let delivery_status = self
            .producer
            .send(
                record,
                Duration::from_millis(self.config.message_timeout_ms as u64),
            )
            .await
            .map_err(|(err, _)| Error::Producer(format!("Failed to send message: {}", err)))?;

        let elapsed = start.elapsed();

        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.record_message_sent(value.len(), elapsed);

        Ok((delivery_status.partition, delivery_status.offset))
    }

    /// Send a batch of messages
    pub async fn send_batch(
        &self,
        topic: &str,
        messages: Vec<(Option<Vec<u8>>, Vec<u8>)>,
    ) -> Result<Vec<Result<(i32, i64)>>> {
        utils::validate_topic_name(topic)?;

        let start = std::time::Instant::now();
        let mut results = Vec::with_capacity(messages.len());

        for (key, value) in messages {
            let result = self.send(topic, key.as_deref(), &value).await;
            results.push(result);
        }

        let elapsed = start.elapsed();
        debug!("Batch of {} messages sent in {:?}", results.len(), elapsed);

        Ok(results)
    }

    /// Flush all pending messages
    pub async fn flush(&self, timeout: Duration) -> Result<()> {
        self.producer
            .flush(timeout)
            .map_err(|e| Error::Producer(format!("Failed to flush producer: {}", e)))?;
        Ok(())
    }

    /// Get the number of partitions for a topic
    async fn get_partition_count(&self, topic: &str) -> Result<i32> {
        let metadata = self
            .producer
            .client()
            .fetch_metadata(Some(topic), Duration::from_secs(10))
            .map_err(|e| Error::KafkaClient(format!("Failed to fetch metadata: {}", e)))?;

        // Find the topic in the metadata
        for topic_metadata in metadata.topics() {
            if topic_metadata.name() == topic {
                return Ok(topic_metadata.partitions().len() as i32);
            }
        }

        Err(Error::KafkaClient(format!(
            "Topic '{}' not found in metadata",
            topic
        )))
    }

    /// Get producer metrics
    pub async fn metrics(&self) -> ProducerMetrics {
        self.metrics.read().await.clone()
    }

    /// Get producer configuration
    pub fn config(&self) -> &ProducerConfig {
        &self.config
    }
}

impl Drop for KafkaProducer {
    fn drop(&mut self) {
        info!("Dropping Kafka producer, flushing remaining messages");
        // Best effort flush on drop
        let _ = self.producer.flush(Duration::from_secs(5));
    }
}

/// Trait for message serialization
#[async_trait]
pub trait MessageSerializer: Send + Sync {
    /// Serialize a message to bytes
    async fn serialize<T>(&self, value: &T) -> Result<Bytes>
    where
        T: serde::Serialize + Send + Sync;
}

/// JSON message serializer
pub struct JsonSerializer;

#[async_trait]
impl MessageSerializer for JsonSerializer {
    async fn serialize<T>(&self, value: &T) -> Result<Bytes>
    where
        T: serde::Serialize + Send + Sync,
    {
        let json = serde_json::to_vec(value)?;
        Ok(Bytes::from(json))
    }
}

/// Message builder for fluent API
pub struct MessageBuilder<'a> {
    producer: &'a KafkaProducer,
    topic: String,
    key: Option<Vec<u8>>,
    value: Vec<u8>,
    headers: Vec<(String, Vec<u8>)>,
    partition: Option<i32>,
}

impl<'a> MessageBuilder<'a> {
    /// Create a new message builder
    pub fn new(producer: &'a KafkaProducer, topic: impl Into<String>) -> Self {
        Self {
            producer,
            topic: topic.into(),
            key: None,
            value: Vec::new(),
            headers: Vec::new(),
            partition: None,
        }
    }

    /// Set message key
    pub fn key(mut self, key: impl Into<Vec<u8>>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Set message value
    pub fn value(mut self, value: impl Into<Vec<u8>>) -> Self {
        self.value = value.into();
        self
    }

    /// Add a header
    pub fn header(mut self, key: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    /// Set partition
    pub fn partition(mut self, partition: i32) -> Self {
        self.partition = Some(partition);
        self
    }

    /// Send the message
    pub async fn send(self) -> Result<(i32, i64)> {
        if self.headers.is_empty() {
            self.producer
                .send(&self.topic, self.key.as_deref(), &self.value)
                .await
        } else {
            self.producer
                .send_with_headers(&self.topic, self.key.as_deref(), &self.value, self.headers)
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_producer_creation() {
        let config = ProducerConfig::builder()
            .bootstrap_servers("localhost:9092")
            .build();

        // We can't actually test the producer without a running Kafka instance
        // but we can test configuration validation
        assert!(config.is_ok());
    }

    #[test]
    fn test_message_builder() {
        // Test builder pattern compilation by creating a function that uses the builder
        fn test_builder(producer: &KafkaProducer) -> MessageBuilder<'_> {
            MessageBuilder::new(producer, "test-topic")
                .key(b"key".to_vec())
                .value(b"value".to_vec())
                .header("header1", b"value1".to_vec())
        }
        // Just verify the function compiles correctly
        let _ = test_builder;
    }
}

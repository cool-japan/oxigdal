//! Kafka consumer implementation

mod config;
mod metrics;
mod offset;
mod rebalance;

pub use config::{ConsumerConfig, ConsumerConfigBuilder};
pub use metrics::{AtomicConsumerMetrics, ConsumerMetrics, ConsumerStats};
pub use offset::{
    CommitStrategy, InMemoryOffsetStore, OffsetCheckpoint, OffsetManager, OffsetStore,
};
pub use rebalance::{
    CustomRebalanceListener, DefaultRebalanceListener, OffsetSavingRebalanceListener,
    RebalanceCallback, RebalanceListenerBuilder, StatefulRebalanceListener,
};

use crate::error::{Error, Result};
use crate::utils;

use bytes::Bytes;
use rdkafka::config::RDKafkaLogLevel;
use rdkafka::consumer::{BaseConsumer, Consumer, ConsumerContext, Rebalance, StreamConsumer};
use rdkafka::message::{BorrowedMessage, Headers};
use rdkafka::{ClientConfig, ClientContext, Message, TopicPartitionList};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

use futures::StreamExt;

/// Kafka consumer for async message consumption
pub struct KafkaConsumer {
    /// Internal rdkafka consumer
    consumer: Arc<StreamConsumer<CustomContext>>,
    /// Consumer configuration
    config: ConsumerConfig,
    /// Offset manager
    offset_manager: Arc<OffsetManager>,
    /// Consumer metrics
    metrics: Arc<RwLock<ConsumerMetrics>>,
    /// Subscribed topics
    subscribed_topics: Arc<RwLock<Vec<String>>>,
    /// Background task driving `CommitStrategy::AutoInterval` commits, if configured.
    auto_commit_task: Option<tokio::task::JoinHandle<()>>,
}

/// Custom consumer context for handling callbacks
struct CustomContext {
    rebalance_callback: Option<Arc<dyn RebalanceCallback>>,
}

impl ClientContext for CustomContext {
    fn log(&self, level: RDKafkaLogLevel, fac: &str, log_message: &str) {
        match level {
            RDKafkaLogLevel::Emerg
            | RDKafkaLogLevel::Alert
            | RDKafkaLogLevel::Critical
            | RDKafkaLogLevel::Error => {
                error!(target: "rdkafka", "{} {}", fac, log_message);
            }
            RDKafkaLogLevel::Warning => {
                warn!(target: "rdkafka", "{} {}", fac, log_message);
            }
            RDKafkaLogLevel::Notice | RDKafkaLogLevel::Info => {
                info!(target: "rdkafka", "{} {}", fac, log_message);
            }
            RDKafkaLogLevel::Debug => {
                debug!(target: "rdkafka", "{} {}", fac, log_message);
            }
        }
    }
}

impl ConsumerContext for CustomContext {
    fn pre_rebalance(&self, _consumer: &BaseConsumer<Self>, rebalance: &Rebalance) {
        info!("Pre-rebalance: {:?}", rebalance);
        if let Some(ref callback) = self.rebalance_callback {
            match rebalance {
                Rebalance::Revoke(tpl) => {
                    callback.on_partitions_revoked(tpl);
                }
                Rebalance::Assign(tpl) => {
                    callback.on_partitions_assigned(tpl);
                }
                Rebalance::Error(err) => {
                    error!("Rebalance error: {}", err);
                }
            }
        }
    }

    fn post_rebalance(&self, _consumer: &BaseConsumer<Self>, rebalance: &Rebalance) {
        info!("Post-rebalance: {:?}", rebalance);
    }
}

impl KafkaConsumer {
    /// Create a new Kafka consumer
    pub async fn new(config: ConsumerConfig) -> Result<Self> {
        Self::new_with_callback(config, None).await
    }

    /// Create a new Kafka consumer with rebalance callback
    pub async fn new_with_callback(
        config: ConsumerConfig,
        rebalance_callback: Option<Arc<dyn RebalanceCallback>>,
    ) -> Result<Self> {
        info!("Creating Kafka consumer with config: {:?}", config);

        let mut client_config = ClientConfig::new();
        config
            .base_config
            .apply_to_client_config(&mut client_config);

        // Consumer-specific configuration
        client_config.set("group.id", &config.group_id);
        client_config.set("enable.auto.commit", config.enable_auto_commit.to_string());
        client_config.set(
            "auto.commit.interval.ms",
            config.auto_commit_interval_ms.to_string(),
        );
        client_config.set(
            "auto.offset.reset",
            config.auto_offset_reset.to_rdkafka_str(),
        );
        client_config.set("isolation.level", config.isolation_level.to_rdkafka_str());
        client_config.set(
            "max.poll.interval.ms",
            config.max_poll_interval_ms.to_string(),
        );
        client_config.set("session.timeout.ms", config.session_timeout_ms.to_string());
        client_config.set(
            "heartbeat.interval.ms",
            config.heartbeat_interval_ms.to_string(),
        );
        client_config.set("fetch.min.bytes", config.fetch_min_bytes.to_string());
        client_config.set("fetch.max.bytes", config.fetch_max_bytes.to_string());
        client_config.set("fetch.wait.max.ms", config.fetch_wait_max_ms.to_string());

        if let Some(max_partition_fetch_bytes) = config.max_partition_fetch_bytes {
            client_config.set(
                "max.partition.fetch.bytes",
                max_partition_fetch_bytes.to_string(),
            );
        }

        let context = CustomContext { rebalance_callback };

        let consumer: StreamConsumer<CustomContext> = client_config
            .create_with_context(context)
            .map_err(|e| Error::Configuration(format!("Failed to create consumer: {}", e)))?;

        let offset_manager = Arc::new(OffsetManager::new(config.commit_strategy.clone()));
        let consumer = Arc::new(consumer);

        // `AutoInterval` is the only strategy that isn't driven directly by
        // `receive_with_timeout`/`receive_batch`; run a background ticker
        // that periodically flushes any tracked-but-uncommitted offsets.
        let auto_commit_task = if config.commit_strategy == CommitStrategy::AutoInterval {
            let interval_ms = u64::from(config.auto_commit_interval_ms.max(1));
            let bg_consumer = Arc::clone(&consumer);
            let bg_offset_manager = Arc::clone(&offset_manager);
            Some(tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_millis(interval_ms));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                // The first tick fires immediately; skip it so the first
                // automatic commit happens after a full interval elapses.
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    if let Err(e) = Self::commit_offsets(&bg_consumer, &bg_offset_manager).await {
                        warn!("Automatic interval offset commit failed: {}", e);
                    }
                }
            }))
        } else {
            None
        };

        info!("Kafka consumer created successfully");

        Ok(Self {
            consumer,
            config,
            offset_manager,
            metrics: Arc::new(RwLock::new(ConsumerMetrics::default())),
            subscribed_topics: Arc::new(RwLock::new(Vec::new())),
            auto_commit_task,
        })
    }

    /// Subscribe to topics
    pub async fn subscribe(&self, topics: &[&str]) -> Result<()> {
        for topic in topics {
            utils::validate_topic_name(topic)?;
        }

        self.consumer
            .subscribe(topics)
            .map_err(|e| Error::Consumer(format!("Failed to subscribe: {}", e)))?;

        let mut subscribed = self.subscribed_topics.write().await;
        *subscribed = topics.iter().map(|s| s.to_string()).collect();

        info!("Subscribed to topics: {:?}", topics);
        Ok(())
    }

    /// Receive a single message
    pub async fn receive(&self) -> Result<KafkaMessage> {
        self.receive_with_timeout(Duration::from_secs(30)).await
    }

    /// Receive a single message with timeout
    pub async fn receive_with_timeout(&self, duration: Duration) -> Result<KafkaMessage> {
        let start = std::time::Instant::now();

        let msg = timeout(duration, self.consumer.stream().next())
            .await
            .map_err(|_| Error::Timeout("Message receive timeout".to_string()))?
            .ok_or_else(|| Error::Consumer("Stream ended".to_string()))?
            .map_err(|e| Error::Consumer(format!("Failed to receive message: {}", e)))?;

        let kafka_msg = Self::convert_message(&msg)?;

        let elapsed = start.elapsed();

        // Update metrics
        let payload_len = kafka_msg.payload.as_ref().map_or(0, |p| p.len());
        let mut metrics = self.metrics.write().await;
        metrics.record_message_received(payload_len, elapsed);
        drop(metrics);

        // Track offset
        self.offset_manager
            .track_offset(&kafka_msg.topic, kafka_msg.partition, kafka_msg.offset);

        // `AutoPerMessage` commits right after every message. `AutoPerBatch`
        // is handled by `receive_batch` once a whole batch completes, and
        // `AutoInterval` is driven by the background task started in
        // `new_with_callback`, so neither is triggered here.
        if matches!(
            self.offset_manager.strategy(),
            CommitStrategy::AutoPerMessage
        ) && self.offset_manager.should_commit(1)
        {
            self.commit().await?;
        }

        Ok(kafka_msg)
    }

    /// Receive messages in batches
    pub async fn receive_batch(
        &self,
        batch_size: usize,
        timeout_duration: Duration,
    ) -> Result<Vec<KafkaMessage>> {
        let mut messages = Vec::with_capacity(batch_size);
        let start = std::time::Instant::now();

        while messages.len() < batch_size && start.elapsed() < timeout_duration {
            let remaining = timeout_duration - start.elapsed();
            match self.receive_with_timeout(remaining).await {
                Ok(msg) => messages.push(msg),
                Err(Error::Timeout(_)) => break,
                Err(e) => return Err(e),
            }
        }

        // `AutoPerBatch` commits once per completed `receive_batch` call
        // rather than after every individual message.
        if matches!(self.offset_manager.strategy(), CommitStrategy::AutoPerBatch)
            && self.offset_manager.should_commit(messages.len())
        {
            self.commit().await?;
        }

        Ok(messages)
    }

    /// Commit offsets manually
    pub async fn commit(&self) -> Result<()> {
        Self::commit_offsets(&self.consumer, &self.offset_manager).await
    }

    /// Commit any tracked-but-uncommitted offsets to the broker.
    ///
    /// Shared between `commit()` and the background `AutoInterval` task.
    async fn commit_offsets(
        consumer: &StreamConsumer<CustomContext>,
        offset_manager: &OffsetManager,
    ) -> Result<()> {
        let offsets = offset_manager.get_offsets_to_commit();
        if offsets.is_empty() {
            return Ok(());
        }

        let mut tpl = TopicPartitionList::new();
        for ((topic, partition), offset) in offsets {
            tpl.add_partition_offset(&topic, partition, rdkafka::Offset::Offset(offset + 1))
                .map_err(|e| Error::OffsetCommit(format!("Failed to add offset: {}", e)))?;
        }

        consumer
            .commit(&tpl, rdkafka::consumer::CommitMode::Sync)
            .map_err(|e| Error::OffsetCommit(format!("Failed to commit offsets: {}", e)))?;

        offset_manager.mark_committed(&tpl);

        debug!("Committed offsets: {:?}", tpl);
        Ok(())
    }

    /// Commit specific offset
    pub async fn commit_offset(&self, topic: &str, partition: i32, offset: i64) -> Result<()> {
        let mut tpl = TopicPartitionList::new();
        tpl.add_partition_offset(topic, partition, rdkafka::Offset::Offset(offset + 1))
            .map_err(|e| Error::OffsetCommit(format!("Failed to add offset: {}", e)))?;

        self.consumer
            .commit(&tpl, rdkafka::consumer::CommitMode::Sync)
            .map_err(|e| Error::OffsetCommit(format!("Failed to commit offset: {}", e)))?;

        Ok(())
    }

    /// Seek to a specific offset
    pub async fn seek(&self, topic: &str, partition: i32, offset: i64) -> Result<()> {
        self.consumer
            .seek(
                topic,
                partition,
                rdkafka::Offset::Offset(offset),
                Duration::from_secs(10),
            )
            .map_err(|e| Error::Consumer(format!("Failed to seek: {}", e)))?;

        Ok(())
    }

    /// Get current assignment
    pub async fn assignment(&self) -> Result<Vec<(String, i32)>> {
        let assignment = self
            .consumer
            .assignment()
            .map_err(|e| Error::Consumer(format!("Failed to get assignment: {}", e)))?;

        let mut result = Vec::new();
        for elem in assignment.elements() {
            result.push((elem.topic().to_string(), elem.partition()));
        }

        Ok(result)
    }

    /// Get committed offsets
    pub async fn committed_offsets(
        &self,
        topic: &str,
        partitions: &[i32],
    ) -> Result<HashMap<i32, i64>> {
        let mut tpl = TopicPartitionList::new();
        for &partition in partitions {
            tpl.add_partition(topic, partition);
        }

        let committed = self
            .consumer
            .committed_offsets(tpl, Duration::from_secs(10))
            .map_err(|e| Error::Consumer(format!("Failed to get committed offsets: {}", e)))?;

        let mut result = HashMap::new();
        for elem in committed.elements() {
            if let rdkafka::Offset::Offset(offset) = elem.offset() {
                result.insert(elem.partition(), offset);
            }
        }

        Ok(result)
    }

    /// Get consumer metrics
    pub async fn metrics(&self) -> ConsumerMetrics {
        self.metrics.read().await.clone()
    }

    /// Get consumer configuration
    pub fn config(&self) -> &ConsumerConfig {
        &self.config
    }

    /// Convert rdkafka message to KafkaMessage
    fn convert_message(msg: &BorrowedMessage) -> Result<KafkaMessage> {
        let topic = msg.topic().to_string();
        let partition = msg.partition();
        let offset = msg.offset();
        let timestamp = msg.timestamp().to_millis().unwrap_or(0);

        let key = msg.key().map(Bytes::copy_from_slice);
        // Kafka log-compacted topics use a `null` payload as the standard
        // tombstone/delete marker (e.g. Debezium CDC pipelines). Treat it as
        // a valid message with no payload rather than a hard error, so
        // consumers of compacted topics can progress past deletes instead
        // of retrying the same offset forever.
        let payload = msg.payload().map(Bytes::copy_from_slice);

        let mut headers = HashMap::new();
        if let Some(msg_headers) = msg.headers() {
            for i in 0..msg_headers.count() {
                let header = msg_headers.get(i);
                headers.insert(
                    header.key.to_string(),
                    Bytes::copy_from_slice(header.value.unwrap_or(&[])),
                );
            }
        }

        Ok(KafkaMessage {
            topic,
            partition,
            offset,
            timestamp,
            key,
            payload,
            headers,
        })
    }

    /// Close the consumer
    pub async fn close(self) -> Result<()> {
        info!("Closing Kafka consumer");
        Ok(())
    }
}

impl Drop for KafkaConsumer {
    fn drop(&mut self) {
        // Stop the background `AutoInterval` commit ticker, if one was
        // started, so it doesn't keep running (and holding an `Arc` clone
        // of the underlying rdkafka consumer alive) after this consumer is
        // dropped.
        if let Some(handle) = self.auto_commit_task.take() {
            handle.abort();
        }
    }
}

/// Kafka message
#[derive(Debug, Clone)]
pub struct KafkaMessage {
    /// Topic name
    pub topic: String,
    /// Partition number
    pub partition: i32,
    /// Message offset
    pub offset: i64,
    /// Message timestamp in milliseconds
    pub timestamp: i64,
    /// Message key (optional)
    pub key: Option<Bytes>,
    /// Message payload. `None` for log-compaction tombstone records (a
    /// Kafka message with a `null` value), which are a valid, standard way
    /// to signal a delete on compacted topics (e.g. Debezium CDC).
    pub payload: Option<Bytes>,
    /// Message headers
    pub headers: HashMap<String, Bytes>,
}

impl KafkaMessage {
    /// Get header value
    pub fn header(&self, key: &str) -> Option<&Bytes> {
        self.headers.get(key)
    }

    /// Get header value as string
    pub fn header_str(&self, key: &str) -> Option<Result<String>> {
        self.headers.get(key).map(|v| {
            String::from_utf8(v.to_vec())
                .map_err(|e| Error::Deserialization(format!("Invalid UTF-8 in header: {}", e)))
        })
    }

    /// Get key as string
    pub fn key_str(&self) -> Option<Result<String>> {
        self.key.as_ref().map(|k| {
            String::from_utf8(k.to_vec())
                .map_err(|e| Error::Deserialization(format!("Invalid UTF-8 in key: {}", e)))
        })
    }

    /// Returns `true` if this is a log-compaction tombstone record (a
    /// Kafka message with a `null` payload, used to signal a delete on
    /// compacted topics).
    pub fn is_tombstone(&self) -> bool {
        self.payload.is_none()
    }

    /// Get payload as string.
    ///
    /// Returns `Ok(None)` for tombstone messages (see [`Self::is_tombstone`]).
    pub fn payload_str(&self) -> Result<Option<String>> {
        self.payload
            .as_ref()
            .map(|payload| {
                String::from_utf8(payload.to_vec())
                    .map_err(|e| Error::Deserialization(format!("Invalid UTF-8 in payload: {}", e)))
            })
            .transpose()
    }

    /// Deserialize payload as JSON.
    ///
    /// Returns `Ok(None)` for tombstone messages (see [`Self::is_tombstone`]).
    pub fn payload_json<T: serde::de::DeserializeOwned>(&self) -> Result<Option<T>> {
        self.payload
            .as_ref()
            .map(|payload| {
                serde_json::from_slice(payload).map_err(|e| {
                    Error::Deserialization(format!("Failed to deserialize JSON: {}", e))
                })
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kafka_message() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), Bytes::from("application/json"));

        let msg = KafkaMessage {
            topic: "test-topic".to_string(),
            partition: 0,
            offset: 123,
            timestamp: 1234567890,
            key: Some(Bytes::from("test-key")),
            payload: Some(Bytes::from("test-payload")),
            headers,
        };

        assert_eq!(msg.topic, "test-topic");
        assert_eq!(msg.partition, 0);
        assert_eq!(msg.offset, 123);
        assert!(msg.header("content-type").is_some());
        assert!(msg.header("missing").is_none());
        assert!(!msg.is_tombstone());
        assert_eq!(
            msg.payload_str().ok().flatten(),
            Some("test-payload".to_string())
        );
    }

    #[test]
    fn test_kafka_message_tombstone() {
        // Log-compaction tombstone: a Kafka record with a `null` value.
        let msg = KafkaMessage {
            topic: "compacted-topic".to_string(),
            partition: 0,
            offset: 42,
            timestamp: 1234567890,
            key: Some(Bytes::from("deleted-key")),
            payload: None,
            headers: HashMap::new(),
        };

        assert!(msg.is_tombstone());
        assert_eq!(
            msg.payload_str().expect("tombstone payload_str is Ok"),
            None
        );
        assert_eq!(
            msg.payload_json::<serde_json::Value>()
                .expect("tombstone payload_json is Ok"),
            None
        );
    }

    #[test]
    fn test_kafka_message_payload_json_non_tombstone() {
        let msg = KafkaMessage {
            topic: "test-topic".to_string(),
            partition: 0,
            offset: 1,
            timestamp: 0,
            key: None,
            payload: Some(Bytes::from(r#"{"value":1}"#)),
            headers: HashMap::new(),
        };

        let value: serde_json::Value = msg
            .payload_json()
            .expect("payload_json should succeed")
            .expect("payload should be present");
        assert_eq!(value["value"], 1);
    }

    #[tokio::test]
    async fn test_consumer_creation() {
        let config = ConsumerConfig::builder()
            .bootstrap_servers("localhost:9092")
            .group_id("test-group")
            .build();

        // We can't actually test the consumer without a running Kafka instance
        // but we can test configuration validation
        assert!(config.is_ok());
    }
}

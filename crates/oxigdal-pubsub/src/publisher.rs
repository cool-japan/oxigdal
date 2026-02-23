//! Publisher module for Google Cloud Pub/Sub.
//!
//! This module provides functionality for publishing messages to Pub/Sub topics
//! with support for batching, ordering keys, retry logic, and flow control.

use crate::error::{PubSubError, Result};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use google_cloud_pubsub::client::{Client, ClientConfig};
use google_cloud_pubsub::publisher::Publisher as GcpPublisher;
use google_cloud_pubsub::topic::Topic;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

/// Maximum message size in bytes (10 MB as per Pub/Sub limits).
pub const MAX_MESSAGE_SIZE: usize = 10_000_000;

/// Default batch size for publishing.
pub const DEFAULT_BATCH_SIZE: usize = 100;

/// Default batch timeout in milliseconds.
pub const DEFAULT_BATCH_TIMEOUT_MS: u64 = 10;

/// Default maximum concurrent publish requests.
pub const DEFAULT_MAX_OUTSTANDING_PUBLISHES: usize = 1000;

/// Message to be published to Pub/Sub.
#[derive(Debug, Clone)]
pub struct Message {
    /// Message data payload.
    pub data: Bytes,
    /// Optional message attributes.
    pub attributes: HashMap<String, String>,
    /// Optional ordering key for ordered delivery.
    pub ordering_key: Option<String>,
}

impl Message {
    /// Creates a new message with the given data.
    pub fn new(data: impl Into<Bytes>) -> Self {
        Self {
            data: data.into(),
            attributes: HashMap::new(),
            ordering_key: None,
        }
    }

    /// Sets an attribute on the message.
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Sets multiple attributes on the message.
    pub fn with_attributes(mut self, attributes: HashMap<String, String>) -> Self {
        self.attributes.extend(attributes);
        self
    }

    /// Sets the ordering key for ordered delivery.
    pub fn with_ordering_key(mut self, ordering_key: impl Into<String>) -> Self {
        self.ordering_key = Some(ordering_key.into());
        self
    }

    /// Validates the message.
    fn validate(&self) -> Result<()> {
        if self.data.is_empty() {
            return Err(PubSubError::InvalidMessageFormat {
                message: "Message data cannot be empty".to_string(),
            });
        }

        if self.data.len() > MAX_MESSAGE_SIZE {
            return Err(PubSubError::message_too_large(
                self.data.len(),
                MAX_MESSAGE_SIZE,
            ));
        }

        Ok(())
    }

    /// Gets the size of the message in bytes.
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

/// Configuration for the publisher.
#[derive(Debug, Clone)]
pub struct PublisherConfig {
    /// Project ID.
    pub project_id: String,
    /// Topic name.
    pub topic_name: String,
    /// Enable message batching.
    pub enable_batching: bool,
    /// Maximum number of messages in a batch.
    pub batch_size: usize,
    /// Maximum time to wait before publishing a batch (milliseconds).
    pub batch_timeout_ms: u64,
    /// Maximum number of outstanding publish requests.
    pub max_outstanding_publishes: usize,
    /// Enable ordering keys.
    pub enable_ordering: bool,
    /// Retry configuration.
    pub retry_config: RetryConfig,
    /// Custom endpoint (for testing).
    pub endpoint: Option<String>,
}

impl Default for PublisherConfig {
    fn default() -> Self {
        Self {
            project_id: String::new(),
            topic_name: String::new(),
            enable_batching: true,
            batch_size: DEFAULT_BATCH_SIZE,
            batch_timeout_ms: DEFAULT_BATCH_TIMEOUT_MS,
            max_outstanding_publishes: DEFAULT_MAX_OUTSTANDING_PUBLISHES,
            enable_ordering: false,
            retry_config: RetryConfig::default(),
            endpoint: None,
        }
    }
}

impl PublisherConfig {
    /// Creates a new publisher configuration.
    pub fn new(project_id: impl Into<String>, topic_name: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            topic_name: topic_name.into(),
            ..Default::default()
        }
    }

    /// Sets whether batching is enabled.
    pub fn with_batching(mut self, enable: bool) -> Self {
        self.enable_batching = enable;
        self
    }

    /// Sets the batch size.
    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    /// Sets the batch timeout.
    pub fn with_batch_timeout(mut self, timeout_ms: u64) -> Self {
        self.batch_timeout_ms = timeout_ms;
        self
    }

    /// Sets the maximum outstanding publishes.
    pub fn with_max_outstanding_publishes(mut self, max: usize) -> Self {
        self.max_outstanding_publishes = max;
        self
    }

    /// Enables ordering keys.
    pub fn with_ordering(mut self, enable: bool) -> Self {
        self.enable_ordering = enable;
        self
    }

    /// Sets the retry configuration.
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Sets a custom endpoint.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Validates the configuration.
    fn validate(&self) -> Result<()> {
        if self.project_id.is_empty() {
            return Err(PubSubError::configuration(
                "Project ID cannot be empty",
                "project_id",
            ));
        }

        if self.topic_name.is_empty() {
            return Err(PubSubError::configuration(
                "Topic name cannot be empty",
                "topic_name",
            ));
        }

        if self.batch_size == 0 {
            return Err(PubSubError::configuration(
                "Batch size must be greater than 0",
                "batch_size",
            ));
        }

        if self.max_outstanding_publishes == 0 {
            return Err(PubSubError::configuration(
                "Max outstanding publishes must be greater than 0",
                "max_outstanding_publishes",
            ));
        }

        Ok(())
    }
}

/// Retry configuration.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_attempts: usize,
    /// Initial retry delay in milliseconds.
    pub initial_delay_ms: u64,
    /// Maximum retry delay in milliseconds.
    pub max_delay_ms: u64,
    /// Multiplier for exponential backoff.
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay_ms: 100,
            max_delay_ms: 60000,
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryConfig {
    /// Calculates the delay for a given attempt.
    ///
    /// Uses exponential backoff with the configured multiplier.
    /// The delay starts at `initial_delay_ms` and doubles (by default) with each attempt,
    /// up to `max_delay_ms`.
    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        let delay = self.initial_delay_ms as f64 * self.backoff_multiplier.powi(attempt as i32);
        let delay = delay.min(self.max_delay_ms as f64) as u64;
        Duration::from_millis(delay)
    }
}

/// Publisher statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublisherStats {
    /// Total number of messages published.
    pub messages_published: u64,
    /// Total number of bytes published.
    pub bytes_published: u64,
    /// Total number of publish errors.
    pub publish_errors: u64,
    /// Total number of retries.
    pub retries: u64,
    /// Number of messages currently in batches.
    pub messages_in_batches: u64,
    /// Number of outstanding publish requests.
    pub outstanding_publishes: u64,
    /// Last publish timestamp.
    pub last_publish: Option<DateTime<Utc>>,
}

/// Batch of messages to be published together.
struct MessageBatch {
    messages: Vec<Message>,
    total_size: usize,
    created_at: DateTime<Utc>,
}

impl MessageBatch {
    fn new() -> Self {
        Self {
            messages: Vec::new(),
            total_size: 0,
            created_at: Utc::now(),
        }
    }

    fn add(&mut self, message: Message) {
        self.total_size += message.size();
        self.messages.push(message);
    }

    fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    fn len(&self) -> usize {
        self.messages.len()
    }

    fn should_flush(&self, max_size: usize, max_age: Duration) -> bool {
        self.len() >= max_size
            || Utc::now()
                .signed_duration_since(self.created_at)
                .to_std()
                .map(|age| age >= max_age)
                .unwrap_or(false)
    }
}

/// Publisher for Google Cloud Pub/Sub.
pub struct Publisher {
    /// Publisher configuration.
    config: PublisherConfig,
    /// Pub/Sub client (kept for ownership).
    #[allow(dead_code)]
    client: Arc<Client>,
    /// Topic handle (kept for ownership).
    #[allow(dead_code)]
    topic: Arc<Topic>,
    /// The underlying GCP publisher.
    publisher: Arc<GcpPublisher>,
    /// Publisher statistics.
    stats: Arc<RwLock<PublisherStats>>,
    /// Message batches keyed by ordering key.
    batches: Arc<DashMap<String, MessageBatch>>,
    /// Semaphore for flow control.
    semaphore: Arc<Semaphore>,
}

impl Publisher {
    /// Creates a new publisher.
    pub async fn new(config: PublisherConfig) -> Result<Self> {
        config.validate()?;

        info!(
            "Creating publisher for topic: {}/{}",
            config.project_id, config.topic_name
        );

        let mut client_config = ClientConfig {
            project_id: Some(config.project_id.clone()),
            ..Default::default()
        };

        if let Some(endpoint) = &config.endpoint {
            client_config.endpoint = endpoint.clone();
        }

        // Initialize authentication if not using emulator
        #[cfg(feature = "auth")]
        let client_config = client_config.with_auth().await.map_err(|e| {
            PubSubError::configuration(
                format!("Failed to initialize authentication: {}", e),
                "authentication",
            )
        })?;

        let client = Client::new(client_config).await.map_err(|e| {
            PubSubError::publish_with_source("Failed to create Pub/Sub client", Box::new(e))
        })?;

        let topic = client.topic(&config.topic_name);

        let publisher_config = google_cloud_pubsub::publisher::PublisherConfig {
            ..Default::default()
        };

        let publisher = topic.new_publisher(Some(publisher_config));

        let semaphore = Arc::new(Semaphore::new(config.max_outstanding_publishes));

        Ok(Self {
            config,
            client: Arc::new(client),
            topic: Arc::new(topic),
            publisher: Arc::new(publisher),
            stats: Arc::new(RwLock::new(PublisherStats::default())),
            batches: Arc::new(DashMap::new()),
            semaphore,
        })
    }

    /// Publishes a single message.
    pub async fn publish(&self, message: Message) -> Result<String> {
        message.validate()?;

        debug!("Publishing message to topic: {}", self.config.topic_name);

        if self.config.enable_batching {
            self.publish_batched(message).await
        } else {
            self.publish_immediate(message).await
        }
    }

    /// Publishes multiple messages.
    pub async fn publish_batch(&self, messages: Vec<Message>) -> Result<Vec<String>> {
        if messages.is_empty() {
            return Ok(Vec::new());
        }

        info!("Publishing batch of {} messages", messages.len());

        let mut results = Vec::with_capacity(messages.len());
        for message in messages {
            let result = self.publish(message).await?;
            results.push(result);
        }

        Ok(results)
    }

    /// Publishes a message immediately without batching.
    async fn publish_immediate(&self, message: Message) -> Result<String> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| PubSubError::publish(format!("Failed to acquire semaphore: {}", e)))?;

        let mut attempt = 0;
        loop {
            match self.publish_with_retry(&message, attempt).await {
                Ok(message_id) => {
                    self.update_stats(message.size(), false);
                    return Ok(message_id);
                }
                Err(e) if e.is_retryable() && attempt < self.config.retry_config.max_attempts => {
                    attempt += 1;
                    let delay = self.config.retry_config.delay_for_attempt(attempt);
                    warn!(
                        "Publish failed, retrying in {:?} (attempt {}/{}): {}",
                        delay, attempt, self.config.retry_config.max_attempts, e
                    );
                    tokio::time::sleep(delay).await;
                    self.stats.write().retries += 1;
                }
                Err(e) => {
                    error!("Publish failed: {}", e);
                    self.stats.write().publish_errors += 1;
                    return Err(e);
                }
            }
        }
    }

    /// Publishes a message with retry logic.
    async fn publish_with_retry(&self, message: &Message, _attempt: usize) -> Result<String> {
        let pubsub_message = google_cloud_googleapis::pubsub::v1::PubsubMessage {
            data: message.data.to_vec(),
            attributes: message.attributes.clone(),
            ordering_key: message.ordering_key.clone().unwrap_or_default(),
            ..Default::default()
        };

        let awaiter = self.publisher.publish(pubsub_message).await;

        let message_id = awaiter.get().await.map_err(|e| {
            PubSubError::publish_with_source("Failed to publish message", Box::new(e))
        })?;

        Ok(message_id)
    }

    /// Publishes a message using batching.
    async fn publish_batched(&self, message: Message) -> Result<String> {
        let key = message
            .ordering_key
            .clone()
            .unwrap_or_else(|| "default".to_string());

        let should_flush = {
            let mut batch_entry = self
                .batches
                .entry(key.clone())
                .or_insert_with(MessageBatch::new);
            batch_entry.add(message.clone());

            batch_entry.should_flush(
                self.config.batch_size,
                Duration::from_millis(self.config.batch_timeout_ms),
            )
        };

        if should_flush {
            self.flush_batch(&key).await?;
        }

        // For batched publishing, we return a placeholder ID
        // In a real implementation, this would track the actual message ID
        Ok(format!("batched-{}", uuid::Uuid::new_v4()))
    }

    /// Flushes a batch of messages.
    async fn flush_batch(&self, key: &str) -> Result<()> {
        let batch = self
            .batches
            .remove(key)
            .map(|(_, batch)| batch)
            .ok_or_else(|| PubSubError::batching("Batch not found", 0))?;

        if batch.is_empty() {
            return Ok(());
        }

        debug!("Flushing batch of {} messages", batch.len());

        for message in batch.messages {
            self.publish_immediate(message).await?;
        }

        Ok(())
    }

    /// Flushes all pending batches.
    pub async fn flush_all(&self) -> Result<()> {
        info!("Flushing all pending batches");

        let keys: Vec<String> = self
            .batches
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        for key in keys {
            if let Err(e) = self.flush_batch(&key).await {
                error!("Failed to flush batch for key {}: {}", key, e);
            }
        }

        Ok(())
    }

    /// Updates publisher statistics.
    fn update_stats(&self, bytes: usize, is_batch: bool) {
        let mut stats = self.stats.write();
        stats.messages_published += 1;
        stats.bytes_published += bytes as u64;
        stats.last_publish = Some(Utc::now());
        if is_batch {
            stats.messages_in_batches = stats.messages_in_batches.saturating_sub(1);
        }
    }

    /// Gets the current publisher statistics.
    pub fn stats(&self) -> PublisherStats {
        self.stats.read().clone()
    }

    /// Resets the publisher statistics.
    pub fn reset_stats(&self) {
        *self.stats.write() = PublisherStats::default();
    }

    /// Gets the topic name.
    pub fn topic_name(&self) -> &str {
        &self.config.topic_name
    }

    /// Gets the project ID.
    pub fn project_id(&self) -> &str {
        &self.config.project_id
    }

    /// Checks if ordering is enabled.
    pub fn is_ordering_enabled(&self) -> bool {
        self.config.enable_ordering
    }

    /// Checks if batching is enabled.
    pub fn is_batching_enabled(&self) -> bool {
        self.config.enable_batching
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        debug!("Dropping publisher for topic: {}", self.config.topic_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let message = Message::new(b"test data".to_vec())
            .with_attribute("key", "value")
            .with_ordering_key("order-1");

        assert_eq!(message.data.as_ref(), b"test data");
        assert_eq!(message.attributes.get("key"), Some(&"value".to_string()));
        assert_eq!(message.ordering_key, Some("order-1".to_string()));
    }

    #[test]
    fn test_message_validation() {
        let valid_message = Message::new(b"test".to_vec());
        assert!(valid_message.validate().is_ok());

        let empty_message = Message::new(Bytes::new());
        assert!(empty_message.validate().is_err());

        let large_message = Message::new(vec![0u8; MAX_MESSAGE_SIZE + 1]);
        assert!(large_message.validate().is_err());
    }

    #[test]
    fn test_publisher_config() {
        let config = PublisherConfig::new("my-project", "my-topic")
            .with_batching(true)
            .with_batch_size(50)
            .with_ordering(true);

        assert_eq!(config.project_id, "my-project");
        assert_eq!(config.topic_name, "my-topic");
        assert!(config.enable_batching);
        assert_eq!(config.batch_size, 50);
        assert!(config.enable_ordering);
    }

    #[test]
    fn test_config_validation() {
        let invalid_config = PublisherConfig::default();
        assert!(invalid_config.validate().is_err());

        let valid_config = PublisherConfig::new("project", "topic");
        assert!(valid_config.validate().is_ok());
    }

    #[test]
    fn test_retry_config_delay() {
        let config = RetryConfig::default();

        let delay0 = config.delay_for_attempt(0);
        assert_eq!(delay0.as_millis(), 100);

        let delay1 = config.delay_for_attempt(1);
        assert_eq!(delay1.as_millis(), 200);

        let delay2 = config.delay_for_attempt(2);
        assert_eq!(delay2.as_millis(), 400);
    }

    #[test]
    fn test_message_batch() {
        let mut batch = MessageBatch::new();
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);

        let message = Message::new(b"test".to_vec());
        batch.add(message);
        assert!(!batch.is_empty());
        assert_eq!(batch.len(), 1);
        assert_eq!(batch.total_size, 4);
    }

    #[test]
    fn test_publisher_stats() {
        let stats = PublisherStats::default();
        assert_eq!(stats.messages_published, 0);
        assert_eq!(stats.bytes_published, 0);
        assert_eq!(stats.publish_errors, 0);
    }
}

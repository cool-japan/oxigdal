//! MQTT publisher implementation

mod batch;
mod persistence;

pub use batch::{BatchPublisher, BatchPublisherConfig};
pub use persistence::{MessagePersistence, PersistentPublisher};

use crate::client::MqttClient;
use crate::error::{MqttError, Result};
use crate::types::{Message, QoS};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{debug, error, warn};

/// Publisher configuration
#[derive(Debug, Clone)]
pub struct PublisherConfig {
    /// Default QoS level
    pub default_qos: QoS,
    /// Default retain flag
    pub default_retain: bool,
    /// Enable persistence
    pub enable_persistence: bool,
    /// Persistence path
    pub persistence_path: Option<String>,
    /// Maximum concurrent publications
    pub max_concurrent: usize,
    /// Publication timeout
    pub timeout: Duration,
    /// Retry failed publications
    pub retry_failed: bool,
    /// Maximum retry attempts
    pub max_retries: usize,
    /// Retry delay
    pub retry_delay: Duration,
}

impl Default for PublisherConfig {
    fn default() -> Self {
        Self {
            default_qos: QoS::AtMostOnce,
            default_retain: false,
            enable_persistence: false,
            persistence_path: None,
            max_concurrent: 10,
            timeout: Duration::from_secs(30),
            retry_failed: true,
            max_retries: 3,
            retry_delay: Duration::from_secs(1),
        }
    }
}

impl PublisherConfig {
    /// Create new publisher configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set default QoS
    pub fn with_qos(mut self, qos: QoS) -> Self {
        self.default_qos = qos;
        self
    }

    /// Set default retain flag
    pub fn with_retain(mut self, retain: bool) -> Self {
        self.default_retain = retain;
        self
    }

    /// Enable persistence
    pub fn with_persistence(mut self, path: String) -> Self {
        self.enable_persistence = true;
        self.persistence_path = Some(path);
        self
    }

    /// Set maximum concurrent publications
    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent = max;
        self
    }

    /// Set publication timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Enable retry on failure
    pub fn with_retry(mut self, max_retries: usize, delay: Duration) -> Self {
        self.retry_failed = true;
        self.max_retries = max_retries;
        self.retry_delay = delay;
        self
    }
}

/// MQTT publisher
pub struct Publisher {
    /// MQTT client
    client: Arc<MqttClient>,
    /// Configuration
    config: PublisherConfig,
    /// Concurrency semaphore
    semaphore: Arc<Semaphore>,
}

impl Publisher {
    /// Create a new publisher
    pub fn new(client: Arc<MqttClient>, config: PublisherConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent));
        Self {
            client,
            config,
            semaphore,
        }
    }

    /// Publish a message
    pub async fn publish(&self, mut message: Message) -> Result<()> {
        // Apply defaults if not set
        if message.qos == QoS::AtMostOnce && self.config.default_qos != QoS::AtMostOnce {
            message.qos = self.config.default_qos;
        }
        if !message.retain && self.config.default_retain {
            message.retain = self.config.default_retain;
        }

        // Acquire semaphore permit
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| MqttError::Internal(format!("Failed to acquire semaphore: {}", e)))?;

        // Publish with timeout and retries
        let mut retries = 0;
        loop {
            match tokio::time::timeout(self.config.timeout, self.client.publish(message.clone()))
                .await
            {
                Ok(Ok(())) => {
                    debug!("Successfully published message to: {}", message.topic);
                    return Ok(());
                }
                Ok(Err(e)) => {
                    retries += 1;
                    if !self.config.retry_failed || retries > self.config.max_retries {
                        error!("Failed to publish message to {}: {}", message.topic, e);
                        return Err(e);
                    }
                    warn!(
                        "Publish failed (attempt {}/{}): {}, retrying...",
                        retries, self.config.max_retries, e
                    );
                    tokio::time::sleep(self.config.retry_delay).await;
                }
                Err(_) => {
                    retries += 1;
                    if !self.config.retry_failed || retries > self.config.max_retries {
                        return Err(MqttError::Timeout {
                            timeout_ms: self.config.timeout.as_millis() as u64,
                        });
                    }
                    warn!(
                        "Publish timeout (attempt {}/{}), retrying...",
                        retries, self.config.max_retries
                    );
                    tokio::time::sleep(self.config.retry_delay).await;
                }
            }
        }
    }

    /// Publish a message to a topic with default settings
    pub async fn publish_simple(
        &self,
        topic: impl Into<String>,
        payload: impl Into<Vec<u8>>,
    ) -> Result<()> {
        let message = Message::new(topic, payload);
        self.publish(message).await
    }

    /// Publish a message with QoS
    pub async fn publish_qos(
        &self,
        topic: impl Into<String>,
        payload: impl Into<Vec<u8>>,
        qos: QoS,
    ) -> Result<()> {
        let message = Message::new(topic, payload).with_qos(qos);
        self.publish(message).await
    }

    /// Publish a retained message
    pub async fn publish_retained(
        &self,
        topic: impl Into<String>,
        payload: impl Into<Vec<u8>>,
    ) -> Result<()> {
        let message = Message::new(topic, payload).with_retain(true);
        self.publish(message).await
    }

    /// Publish multiple messages concurrently
    pub async fn publish_many(&self, messages: Vec<Message>) -> Result<Vec<Result<()>>> {
        let tasks: Vec<_> = messages
            .into_iter()
            .map(|msg| {
                let publisher = self.clone_ref();
                tokio::spawn(async move { publisher.publish(msg).await })
            })
            .collect();

        let mut results = Vec::new();
        for task in tasks {
            match task.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(Err(MqttError::Internal(format!("Task join error: {}", e)))),
            }
        }

        Ok(results)
    }

    /// Clear a retained message
    pub async fn clear_retained(&self, topic: impl Into<String>) -> Result<()> {
        let message = Message::new(topic, Vec::new()).with_retain(true);
        self.publish(message).await
    }

    /// Get configuration
    pub fn config(&self) -> &PublisherConfig {
        &self.config
    }

    /// Clone with new reference
    fn clone_ref(&self) -> Self {
        Self {
            client: Arc::clone(&self.client),
            config: self.config.clone(),
            semaphore: Arc::clone(&self.semaphore),
        }
    }
}

/// Topic publisher - publishes to a specific topic
pub struct TopicPublisher {
    /// Publisher
    publisher: Arc<Publisher>,
    /// Topic
    topic: String,
    /// Default QoS
    qos: QoS,
    /// Default retain flag
    retain: bool,
}

impl TopicPublisher {
    /// Create a new topic publisher
    pub fn new(publisher: Arc<Publisher>, topic: impl Into<String>) -> Self {
        Self {
            publisher,
            topic: topic.into(),
            qos: QoS::AtMostOnce,
            retain: false,
        }
    }

    /// Set default QoS
    pub fn with_qos(mut self, qos: QoS) -> Self {
        self.qos = qos;
        self
    }

    /// Set default retain flag
    pub fn with_retain(mut self, retain: bool) -> Self {
        self.retain = retain;
        self
    }

    /// Publish a payload
    pub async fn publish(&self, payload: impl Into<Vec<u8>>) -> Result<()> {
        let message = Message::new(self.topic.clone(), payload)
            .with_qos(self.qos)
            .with_retain(self.retain);
        self.publisher.publish(message).await
    }

    /// Publish a string payload
    pub async fn publish_str(&self, payload: &str) -> Result<()> {
        self.publish(payload.as_bytes().to_vec()).await
    }

    /// Publish JSON payload
    pub async fn publish_json<T: serde::Serialize>(&self, payload: &T) -> Result<()> {
        let json = serde_json::to_vec(payload)?;
        self.publish(json).await
    }

    /// Get topic
    pub fn topic(&self) -> &str {
        &self.topic
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::client::{ClientConfig, MqttClient};
    use crate::types::ConnectionOptions;

    #[tokio::test]
    async fn test_publisher_creation() {
        let conn_opts = ConnectionOptions::new("localhost", 1883, "test-pub");
        let client_config = ClientConfig::new(conn_opts);
        let client = MqttClient::new(client_config).expect("Failed to create client");
        let client = Arc::new(client);

        let pub_config = PublisherConfig::new();
        let publisher = Publisher::new(client, pub_config);

        assert_eq!(publisher.config().default_qos, QoS::AtMostOnce);
        assert!(!publisher.config().default_retain);
    }

    #[tokio::test]
    async fn test_message_preparation() {
        let msg = Message::new("test/topic", b"hello".to_vec())
            .with_qos(QoS::AtLeastOnce)
            .with_retain(true);

        assert_eq!(msg.topic, "test/topic");
        assert_eq!(msg.qos, QoS::AtLeastOnce);
        assert!(msg.retain);
    }

    #[test]
    fn test_publisher_config() {
        let config = PublisherConfig::new()
            .with_qos(QoS::ExactlyOnce)
            .with_retain(true)
            .with_max_concurrent(20)
            .with_timeout(Duration::from_secs(60));

        assert_eq!(config.default_qos, QoS::ExactlyOnce);
        assert!(config.default_retain);
        assert_eq!(config.max_concurrent, 20);
        assert_eq!(config.timeout, Duration::from_secs(60));
    }
}

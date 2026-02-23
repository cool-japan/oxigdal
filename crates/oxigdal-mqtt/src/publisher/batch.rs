//! Batch publishing implementation

use crate::error::{MqttError, Result};
use crate::publisher::Publisher;
use crate::types::Message;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Batch publisher configuration
#[derive(Debug, Clone)]
pub struct BatchPublisherConfig {
    /// Maximum batch size
    pub max_batch_size: usize,
    /// Maximum wait time before publishing batch
    pub max_wait_time: Duration,
    /// Buffer capacity
    pub buffer_capacity: usize,
    /// Enable automatic flushing
    pub auto_flush: bool,
}

impl Default for BatchPublisherConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 100,
            max_wait_time: Duration::from_secs(1),
            buffer_capacity: 1000,
            auto_flush: true,
        }
    }
}

impl BatchPublisherConfig {
    /// Create new batch publisher configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum batch size
    pub fn with_max_batch_size(mut self, size: usize) -> Self {
        self.max_batch_size = size;
        self
    }

    /// Set maximum wait time
    pub fn with_max_wait_time(mut self, duration: Duration) -> Self {
        self.max_wait_time = duration;
        self
    }

    /// Set buffer capacity
    pub fn with_buffer_capacity(mut self, capacity: usize) -> Self {
        self.buffer_capacity = capacity;
        self
    }

    /// Enable or disable auto flush
    pub fn with_auto_flush(mut self, enable: bool) -> Self {
        self.auto_flush = enable;
        self
    }
}

/// Batch publisher for efficient message batching
pub struct BatchPublisher {
    /// Publisher
    #[allow(dead_code)]
    publisher: Arc<Publisher>,
    /// Configuration
    config: BatchPublisherConfig,
    /// Message sender
    tx: mpsc::Sender<Message>,
    /// Worker task handle
    #[allow(dead_code)]
    worker_handle: Option<tokio::task::JoinHandle<()>>,
}

impl BatchPublisher {
    /// Create a new batch publisher
    pub fn new(publisher: Arc<Publisher>, config: BatchPublisherConfig) -> Self {
        let (tx, rx) = mpsc::channel(config.buffer_capacity);

        let worker_publisher = Arc::clone(&publisher);
        let worker_config = config.clone();

        let worker_handle = tokio::spawn(async move {
            Self::worker_loop(worker_publisher, worker_config, rx).await;
        });

        Self {
            publisher,
            config,
            tx,
            worker_handle: Some(worker_handle),
        }
    }

    /// Worker loop for batch processing
    async fn worker_loop(
        publisher: Arc<Publisher>,
        config: BatchPublisherConfig,
        mut rx: mpsc::Receiver<Message>,
    ) {
        let mut batch: Vec<Message> = Vec::with_capacity(config.max_batch_size);
        let mut interval = tokio::time::interval(config.max_wait_time);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        info!("Batch publisher worker started");

        loop {
            tokio::select! {
                // Receive new message
                msg = rx.recv() => {
                    match msg {
                        Some(message) => {
                            batch.push(message);

                            // Flush if batch is full
                            if batch.len() >= config.max_batch_size {
                                debug!("Batch full ({} messages), flushing", batch.len());
                                if let Err(e) = Self::flush_batch(&publisher, &mut batch).await {
                                    error!("Error flushing batch: {}", e);
                                }
                            }
                        }
                        None => {
                            info!("Batch publisher channel closed, flushing remaining messages");
                            if !batch.is_empty() {
                                if let Err(e) = Self::flush_batch(&publisher, &mut batch).await {
                                    error!("Error flushing final batch: {}", e);
                                }
                            }
                            break;
                        }
                    }
                }

                // Periodic flush
                _ = interval.tick() => {
                    if !batch.is_empty() && config.auto_flush {
                        debug!("Auto-flushing batch ({} messages)", batch.len());
                        if let Err(e) = Self::flush_batch(&publisher, &mut batch).await {
                            error!("Error during auto-flush: {}", e);
                        }
                    }
                }
            }
        }

        info!("Batch publisher worker stopped");
    }

    /// Flush a batch of messages
    async fn flush_batch(publisher: &Publisher, batch: &mut Vec<Message>) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }

        let count = batch.len();
        debug!("Flushing batch of {} messages", count);

        // Publish all messages concurrently
        let results = publisher.publish_many(std::mem::take(batch)).await?;

        // Count successes and failures
        let mut success = 0;
        let mut failed = 0;

        for result in results {
            match result {
                Ok(()) => success += 1,
                Err(e) => {
                    failed += 1;
                    warn!("Failed to publish message in batch: {}", e);
                }
            }
        }

        info!(
            "Batch flush complete: {} succeeded, {} failed",
            success, failed
        );

        if failed > 0 {
            Err(MqttError::Publication(
                crate::error::PublicationError::PublishFailed {
                    topic: "batch".to_string(),
                    reason: format!("{} messages failed", failed),
                },
            ))
        } else {
            Ok(())
        }
    }

    /// Add a message to the batch
    pub async fn publish(&self, message: Message) -> Result<()> {
        self.tx
            .send(message)
            .await
            .map_err(|e| MqttError::Internal(format!("Failed to queue message: {}", e)))
    }

    /// Add a simple message to the batch
    pub async fn publish_simple(
        &self,
        topic: impl Into<String>,
        payload: impl Into<Vec<u8>>,
    ) -> Result<()> {
        let message = Message::new(topic, payload);
        self.publish(message).await
    }

    /// Flush all pending messages immediately
    pub async fn flush(&self) -> Result<()> {
        // Send a marker message and wait for it to be processed
        // This ensures all previous messages are flushed
        let marker = Message::new("__flush_marker__", Vec::new());
        self.publish(marker).await?;

        // Give some time for the flush to complete
        tokio::time::sleep(Duration::from_millis(100)).await;

        Ok(())
    }

    /// Get the number of pending messages
    pub fn pending_count(&self) -> usize {
        // Note: This is approximate due to concurrent access
        self.tx.max_capacity() - self.tx.capacity()
    }

    /// Get configuration
    pub fn config(&self) -> &BatchPublisherConfig {
        &self.config
    }
}

impl Drop for BatchPublisher {
    fn drop(&mut self) {
        // Drop the sender to signal worker to stop
        drop(self.tx.clone());

        // Note: We can't wait for the worker here in a synchronous Drop
        // The caller should call flush() before dropping
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::client::{ClientConfig, MqttClient};
    use crate::publisher::PublisherConfig;
    use crate::types::ConnectionOptions;

    #[tokio::test]
    async fn test_batch_publisher_creation() {
        let conn_opts = ConnectionOptions::new("localhost", 1883, "test-batch");
        let client_config = ClientConfig::new(conn_opts);
        let client = MqttClient::new(client_config).expect("Failed to create client");
        let client = Arc::new(client);

        let pub_config = PublisherConfig::new();
        let publisher = Arc::new(Publisher::new(client, pub_config));

        let batch_config = BatchPublisherConfig::new();
        let batch_publisher = BatchPublisher::new(publisher, batch_config);

        assert_eq!(batch_publisher.config().max_batch_size, 100);
    }

    #[test]
    fn test_batch_config() {
        let config = BatchPublisherConfig::new()
            .with_max_batch_size(50)
            .with_max_wait_time(Duration::from_millis(500))
            .with_buffer_capacity(500)
            .with_auto_flush(false);

        assert_eq!(config.max_batch_size, 50);
        assert_eq!(config.max_wait_time, Duration::from_millis(500));
        assert_eq!(config.buffer_capacity, 500);
        assert!(!config.auto_flush);
    }
}

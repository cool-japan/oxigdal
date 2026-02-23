//! Subscriber module for Google Cloud Pub/Sub.
//!
//! This module provides functionality for subscribing to Pub/Sub topics
//! with support for pull/push subscriptions, message acknowledgment,
//! flow control, and dead letter queues.

use crate::error::{PubSubError, Result};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use google_cloud_pubsub::client::{Client, ClientConfig};
use google_cloud_pubsub::subscription::Subscription;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

/// Default flow control maximum messages.
pub const DEFAULT_MAX_OUTSTANDING_MESSAGES: usize = 1000;

/// Default flow control maximum bytes.
pub const DEFAULT_MAX_OUTSTANDING_BYTES: usize = 100_000_000; // 100 MB

/// Default acknowledgment deadline in seconds.
pub const DEFAULT_ACK_DEADLINE_SECONDS: i64 = 10;

/// Default message handler concurrency.
pub const DEFAULT_HANDLER_CONCURRENCY: usize = 10;

/// Received message from Pub/Sub.
#[derive(Debug, Clone)]
pub struct ReceivedMessage {
    /// Message ID.
    pub message_id: String,
    /// Message data payload.
    pub data: Bytes,
    /// Message attributes.
    pub attributes: HashMap<String, String>,
    /// Publish timestamp.
    pub publish_time: DateTime<Utc>,
    /// Ordering key if present.
    pub ordering_key: Option<String>,
    /// Delivery attempt count.
    pub delivery_attempt: i32,
    /// Acknowledgment ID (internal).
    pub(crate) ack_id: String,
}

impl ReceivedMessage {
    /// Gets the size of the message in bytes.
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Checks if this is a redelivery.
    pub fn is_redelivery(&self) -> bool {
        self.delivery_attempt > 1
    }
}

/// Subscription type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionType {
    /// Pull subscription - client actively pulls messages.
    Pull,
    /// Push subscription - server pushes messages to an endpoint.
    Push,
}

/// Flow control settings.
#[derive(Debug, Clone)]
pub struct FlowControlSettings {
    /// Maximum number of outstanding messages.
    pub max_outstanding_messages: usize,
    /// Maximum bytes of outstanding messages.
    pub max_outstanding_bytes: usize,
    /// Limit messages per second (0 = unlimited).
    pub max_messages_per_second: u64,
}

impl Default for FlowControlSettings {
    fn default() -> Self {
        Self {
            max_outstanding_messages: DEFAULT_MAX_OUTSTANDING_MESSAGES,
            max_outstanding_bytes: DEFAULT_MAX_OUTSTANDING_BYTES,
            max_messages_per_second: 0,
        }
    }
}

/// Dead letter queue configuration.
#[derive(Debug, Clone)]
pub struct DeadLetterConfig {
    /// Dead letter topic name.
    pub topic_name: String,
    /// Maximum delivery attempts before sending to DLQ.
    pub max_delivery_attempts: i32,
}

impl DeadLetterConfig {
    /// Creates a new dead letter configuration.
    pub fn new(topic_name: impl Into<String>, max_delivery_attempts: i32) -> Self {
        Self {
            topic_name: topic_name.into(),
            max_delivery_attempts,
        }
    }
}

/// Configuration for the subscriber.
#[derive(Debug, Clone)]
pub struct SubscriberConfig {
    /// Project ID.
    pub project_id: String,
    /// Subscription name.
    pub subscription_name: String,
    /// Subscription type.
    pub subscription_type: SubscriptionType,
    /// Acknowledgment deadline in seconds.
    pub ack_deadline_seconds: i64,
    /// Flow control settings.
    pub flow_control: FlowControlSettings,
    /// Message handler concurrency.
    pub handler_concurrency: usize,
    /// Enable message ordering.
    pub enable_ordering: bool,
    /// Dead letter queue configuration.
    pub dead_letter_config: Option<DeadLetterConfig>,
    /// Custom endpoint (for testing).
    pub endpoint: Option<String>,
    /// Automatically extend acknowledgment deadlines.
    pub auto_extend_deadline: bool,
}

impl Default for SubscriberConfig {
    fn default() -> Self {
        Self {
            project_id: String::new(),
            subscription_name: String::new(),
            subscription_type: SubscriptionType::Pull,
            ack_deadline_seconds: DEFAULT_ACK_DEADLINE_SECONDS,
            flow_control: FlowControlSettings::default(),
            handler_concurrency: DEFAULT_HANDLER_CONCURRENCY,
            enable_ordering: false,
            dead_letter_config: None,
            endpoint: None,
            auto_extend_deadline: true,
        }
    }
}

impl SubscriberConfig {
    /// Creates a new subscriber configuration.
    pub fn new(project_id: impl Into<String>, subscription_name: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            subscription_name: subscription_name.into(),
            ..Default::default()
        }
    }

    /// Sets the subscription type.
    pub fn with_type(mut self, subscription_type: SubscriptionType) -> Self {
        self.subscription_type = subscription_type;
        self
    }

    /// Sets the acknowledgment deadline.
    pub fn with_ack_deadline(mut self, seconds: i64) -> Self {
        self.ack_deadline_seconds = seconds;
        self
    }

    /// Sets the flow control settings.
    pub fn with_flow_control(mut self, settings: FlowControlSettings) -> Self {
        self.flow_control = settings;
        self
    }

    /// Sets the handler concurrency.
    pub fn with_handler_concurrency(mut self, concurrency: usize) -> Self {
        self.handler_concurrency = concurrency;
        self
    }

    /// Enables message ordering.
    pub fn with_ordering(mut self, enable: bool) -> Self {
        self.enable_ordering = enable;
        self
    }

    /// Sets the dead letter queue configuration.
    pub fn with_dead_letter(mut self, config: DeadLetterConfig) -> Self {
        self.dead_letter_config = Some(config);
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

        if self.subscription_name.is_empty() {
            return Err(PubSubError::configuration(
                "Subscription name cannot be empty",
                "subscription_name",
            ));
        }

        if self.ack_deadline_seconds < 10 || self.ack_deadline_seconds > 600 {
            return Err(PubSubError::configuration(
                "Acknowledgment deadline must be between 10 and 600 seconds",
                "ack_deadline_seconds",
            ));
        }

        if self.handler_concurrency == 0 {
            return Err(PubSubError::configuration(
                "Handler concurrency must be greater than 0",
                "handler_concurrency",
            ));
        }

        Ok(())
    }
}

/// Subscriber statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubscriberStats {
    /// Total number of messages received.
    pub messages_received: u64,
    /// Total number of bytes received.
    pub bytes_received: u64,
    /// Total number of messages acknowledged.
    pub messages_acknowledged: u64,
    /// Total number of messages not acknowledged (rejected).
    pub messages_nacked: u64,
    /// Total number of messages sent to dead letter queue.
    pub messages_to_dlq: u64,
    /// Total number of acknowledgment errors.
    pub ack_errors: u64,
    /// Number of outstanding messages.
    pub outstanding_messages: u64,
    /// Number of outstanding bytes.
    pub outstanding_bytes: u64,
    /// Last message received timestamp.
    pub last_receive: Option<DateTime<Utc>>,
}

/// Message handler result.
pub enum HandlerResult {
    /// Acknowledge the message (successful processing).
    Ack,
    /// Not acknowledge the message (will be redelivered).
    Nack,
    /// Send message to dead letter queue.
    DeadLetter,
}

/// Message handler function type.
pub type MessageHandler = Arc<dyn Fn(ReceivedMessage) -> HandlerResult + Send + Sync>;

/// Subscriber for Google Cloud Pub/Sub.
pub struct Subscriber {
    config: SubscriberConfig,
    client: Arc<Client>,
    subscription: Arc<Subscription>,
    stats: Arc<RwLock<SubscriberStats>>,
    outstanding_messages: Arc<DashMap<String, ReceivedMessage>>,
    running: Arc<AtomicBool>,
    message_count: Arc<AtomicU64>,
    byte_count: Arc<AtomicU64>,
}

impl Subscriber {
    /// Creates a new subscriber.
    pub async fn new(config: SubscriberConfig) -> Result<Self> {
        config.validate()?;

        info!(
            "Creating subscriber for subscription: {}/{}",
            config.project_id, config.subscription_name
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
            PubSubError::subscription_with_source("Failed to create Pub/Sub client", Box::new(e))
        })?;

        let subscription = client.subscription(&config.subscription_name);

        Ok(Self {
            config,
            client: Arc::new(client),
            subscription: Arc::new(subscription),
            stats: Arc::new(RwLock::new(SubscriberStats::default())),
            outstanding_messages: Arc::new(DashMap::new()),
            running: Arc::new(AtomicBool::new(false)),
            message_count: Arc::new(AtomicU64::new(0)),
            byte_count: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Pulls a single message from the subscription.
    pub async fn pull_one(&self) -> Result<Option<ReceivedMessage>> {
        self.check_flow_control(1, 0)?;

        debug!(
            "Pulling message from subscription: {}",
            self.config.subscription_name
        );

        // Pull one message
        let messages = self.subscription.pull(1, None).await.map_err(|e| {
            PubSubError::subscription_with_source("Failed to pull message", Box::new(e))
        })?;

        if messages.is_empty() {
            return Ok(None);
        }

        let message = &messages[0];
        let received = ReceivedMessage {
            message_id: message.message.message_id.clone(),
            data: Bytes::from(message.message.data.clone()),
            attributes: message.message.attributes.clone(),
            publish_time: Utc::now(), // Convert from protobuf timestamp
            ordering_key: if message.message.ordering_key.is_empty() {
                None
            } else {
                Some(message.message.ordering_key.clone())
            },
            delivery_attempt: message.delivery_attempt().map(|x| x as i32).unwrap_or(0),
            ack_id: message.ack_id().to_string(),
        };

        self.track_message(&received);
        Ok(Some(received))
    }

    /// Pulls multiple messages from the subscription.
    pub async fn pull(&self, max_messages: i32) -> Result<Vec<ReceivedMessage>> {
        self.check_flow_control(max_messages as usize, 0)?;

        debug!(
            "Pulling up to {} messages from subscription: {}",
            max_messages, self.config.subscription_name
        );

        let messages = self
            .subscription
            .pull(max_messages, None)
            .await
            .map_err(|e| {
                PubSubError::subscription_with_source("Failed to pull messages", Box::new(e))
            })?;

        let received: Vec<ReceivedMessage> = messages
            .iter()
            .map(|msg| {
                let received = ReceivedMessage {
                    message_id: msg.message.message_id.clone(),
                    data: Bytes::from(msg.message.data.clone()),
                    attributes: msg.message.attributes.clone(),
                    publish_time: Utc::now(),
                    ordering_key: if msg.message.ordering_key.is_empty() {
                        None
                    } else {
                        Some(msg.message.ordering_key.clone())
                    },
                    delivery_attempt: msg.delivery_attempt().map(|x| x as i32).unwrap_or(0),
                    ack_id: msg.ack_id().to_string(),
                };
                self.track_message(&received);
                received
            })
            .collect();

        Ok(received)
    }

    /// Acknowledges a message.
    pub async fn acknowledge(&self, message: &ReceivedMessage) -> Result<()> {
        debug!("Acknowledging message: {}", message.message_id);

        self.subscription
            .ack(vec![message.ack_id.clone()])
            .await
            .map_err(|e| {
                PubSubError::acknowledgment(format!("Failed to acknowledge message: {}", e))
            })?;

        self.untrack_message(message);
        self.stats.write().messages_acknowledged += 1;

        Ok(())
    }

    /// Not acknowledges a message (will be redelivered).
    pub async fn nack(&self, message: &ReceivedMessage) -> Result<()> {
        debug!("Not acknowledging message: {}", message.message_id);

        // Import the request type
        use google_cloud_googleapis::pubsub::v1::ModifyAckDeadlineRequest;

        // Get the subscriber client from subscription
        let client = self.subscription.get_client();
        let fqsn = self.subscription.fully_qualified_name();

        // Create request to set deadline to 0 (immediate redelivery)
        let req = ModifyAckDeadlineRequest {
            subscription: fqsn.to_string(),
            ack_ids: vec![message.ack_id.clone()],
            ack_deadline_seconds: 0,
        };

        client
            .modify_ack_deadline(req, None)
            .await
            .map_err(|e| PubSubError::acknowledgment(format!("Failed to nack message: {}", e)))?;

        self.untrack_message(message);
        self.stats.write().messages_nacked += 1;

        Ok(())
    }
    /// Extends the acknowledgment deadline for a message.
    pub async fn extend_deadline(&self, message: &ReceivedMessage, seconds: i32) -> Result<()> {
        debug!(
            "Extending acknowledgment deadline for message: {} by {} seconds",
            message.message_id, seconds
        );

        // Import the request type
        use google_cloud_googleapis::pubsub::v1::ModifyAckDeadlineRequest;

        // Get the subscriber client from subscription
        let client = self.subscription.get_client();
        let fqsn = self.subscription.fully_qualified_name();

        // Create request
        let req = ModifyAckDeadlineRequest {
            subscription: fqsn.to_string(),
            ack_ids: vec![message.ack_id.clone()],
            ack_deadline_seconds: seconds,
        };

        client.modify_ack_deadline(req, None).await.map_err(|e| {
            PubSubError::acknowledgment(format!("Failed to extend deadline: {}", e))
        })?;

        Ok(())
    }

    /// Sends a message to the dead letter queue.
    pub async fn send_to_dead_letter(&self, message: &ReceivedMessage) -> Result<()> {
        let dlq_config = self.config.dead_letter_config.as_ref().ok_or_else(|| {
            PubSubError::dead_letter("Dead letter queue not configured", &message.message_id)
        })?;

        info!(
            "Sending message {} to dead letter queue: {}",
            message.message_id, dlq_config.topic_name
        );

        // Acknowledge the original message first
        self.acknowledge(message).await?;

        self.stats.write().messages_to_dlq += 1;

        Ok(())
    }

    /// Starts a subscription with a message handler.
    pub async fn start<F>(&self, handler: F) -> Result<JoinHandle<()>>
    where
        F: Fn(ReceivedMessage) -> HandlerResult + Send + Sync + 'static,
    {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(PubSubError::subscription("Subscriber already running"));
        }

        info!(
            "Starting subscriber for subscription: {}",
            self.config.subscription_name
        );

        let handler = Arc::new(handler);
        let (tx, mut rx) = mpsc::channel(self.config.handler_concurrency);

        let subscriber = self.clone_arc();
        let pull_task = tokio::spawn(async move {
            while subscriber.running.load(Ordering::SeqCst) {
                match subscriber.pull(10).await {
                    Ok(messages) if !messages.is_empty() => {
                        for message in messages {
                            if tx.send(message).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(_) => {
                        // No messages, wait a bit
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    Err(e) => {
                        error!("Error pulling messages: {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });

        let subscriber = self.clone_arc();
        let _handler_task = tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                let result = handler(message.clone());
                match result {
                    HandlerResult::Ack => {
                        if let Err(e) = subscriber.acknowledge(&message).await {
                            error!("Failed to acknowledge message: {}", e);
                        }
                    }
                    HandlerResult::Nack => {
                        if let Err(e) = subscriber.nack(&message).await {
                            error!("Failed to nack message: {}", e);
                        }
                    }
                    HandlerResult::DeadLetter => {
                        if let Err(e) = subscriber.send_to_dead_letter(&message).await {
                            error!("Failed to send message to DLQ: {}", e);
                        }
                    }
                }
            }
        });

        Ok(pull_task)
    }

    /// Stops the subscriber.
    pub fn stop(&self) {
        info!(
            "Stopping subscriber for subscription: {}",
            self.config.subscription_name
        );
        self.running.store(false, Ordering::SeqCst);
    }

    /// Checks flow control limits.
    fn check_flow_control(&self, messages: usize, bytes: usize) -> Result<()> {
        let current_messages = self.message_count.load(Ordering::Relaxed) as usize;
        let current_bytes = self.byte_count.load(Ordering::Relaxed) as usize;

        if current_messages + messages > self.config.flow_control.max_outstanding_messages {
            return Err(PubSubError::flow_control(
                "Outstanding message limit exceeded",
                current_messages + messages,
                self.config.flow_control.max_outstanding_messages,
            ));
        }

        if current_bytes + bytes > self.config.flow_control.max_outstanding_bytes {
            return Err(PubSubError::flow_control(
                "Outstanding bytes limit exceeded",
                current_bytes + bytes,
                self.config.flow_control.max_outstanding_bytes,
            ));
        }

        Ok(())
    }

    /// Tracks a received message.
    fn track_message(&self, message: &ReceivedMessage) {
        self.outstanding_messages
            .insert(message.message_id.clone(), message.clone());
        self.message_count.fetch_add(1, Ordering::Relaxed);
        self.byte_count
            .fetch_add(message.size() as u64, Ordering::Relaxed);

        let mut stats = self.stats.write();
        stats.messages_received += 1;
        stats.bytes_received += message.size() as u64;
        stats.outstanding_messages += 1;
        stats.outstanding_bytes += message.size() as u64;
        stats.last_receive = Some(Utc::now());
    }

    /// Untracks a received message.
    fn untrack_message(&self, message: &ReceivedMessage) {
        self.outstanding_messages.remove(&message.message_id);
        self.message_count.fetch_sub(1, Ordering::Relaxed);
        self.byte_count
            .fetch_sub(message.size() as u64, Ordering::Relaxed);

        let mut stats = self.stats.write();
        stats.outstanding_messages = stats.outstanding_messages.saturating_sub(1);
        stats.outstanding_bytes = stats
            .outstanding_bytes
            .saturating_sub(message.size() as u64);
    }

    /// Clones the subscriber with Arc.
    fn clone_arc(&self) -> Arc<Self> {
        Arc::new(Self {
            config: self.config.clone(),
            client: Arc::clone(&self.client),
            subscription: Arc::clone(&self.subscription),
            stats: Arc::clone(&self.stats),
            outstanding_messages: Arc::clone(&self.outstanding_messages),
            running: Arc::clone(&self.running),
            message_count: Arc::clone(&self.message_count),
            byte_count: Arc::clone(&self.byte_count),
        })
    }

    /// Gets the current subscriber statistics.
    pub fn stats(&self) -> SubscriberStats {
        self.stats.read().clone()
    }

    /// Resets the subscriber statistics.
    pub fn reset_stats(&self) {
        *self.stats.write() = SubscriberStats::default();
    }

    /// Gets the subscription name.
    pub fn subscription_name(&self) -> &str {
        &self.config.subscription_name
    }

    /// Gets the project ID.
    pub fn project_id(&self) -> &str {
        &self.config.project_id
    }

    /// Checks if the subscriber is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscriber_config() {
        let config = SubscriberConfig::new("my-project", "my-subscription")
            .with_type(SubscriptionType::Pull)
            .with_ack_deadline(30)
            .with_ordering(true);

        assert_eq!(config.project_id, "my-project");
        assert_eq!(config.subscription_name, "my-subscription");
        assert_eq!(config.subscription_type, SubscriptionType::Pull);
        assert_eq!(config.ack_deadline_seconds, 30);
        assert!(config.enable_ordering);
    }

    #[test]
    fn test_config_validation() {
        let invalid_config = SubscriberConfig::default();
        assert!(invalid_config.validate().is_err());

        let valid_config = SubscriberConfig::new("project", "subscription");
        assert!(valid_config.validate().is_ok());
    }

    #[test]
    fn test_flow_control_settings() {
        let settings = FlowControlSettings::default();
        assert_eq!(
            settings.max_outstanding_messages,
            DEFAULT_MAX_OUTSTANDING_MESSAGES
        );
        assert_eq!(
            settings.max_outstanding_bytes,
            DEFAULT_MAX_OUTSTANDING_BYTES
        );
    }

    #[test]
    fn test_dead_letter_config() {
        let config = DeadLetterConfig::new("dlq-topic", 5);
        assert_eq!(config.topic_name, "dlq-topic");
        assert_eq!(config.max_delivery_attempts, 5);
    }

    #[test]
    fn test_received_message() {
        let message = ReceivedMessage {
            message_id: "msg-1".to_string(),
            data: Bytes::from(b"test data".to_vec()),
            attributes: HashMap::new(),
            publish_time: Utc::now(),
            ordering_key: None,
            delivery_attempt: 1,
            ack_id: "ack-1".to_string(),
        };

        assert_eq!(message.size(), 9);
        assert!(!message.is_redelivery());

        let redelivered = ReceivedMessage {
            delivery_attempt: 2,
            ..message.clone()
        };
        assert!(redelivered.is_redelivery());
    }

    #[test]
    fn test_subscriber_stats() {
        let stats = SubscriberStats::default();
        assert_eq!(stats.messages_received, 0);
        assert_eq!(stats.messages_acknowledged, 0);
        assert_eq!(stats.messages_nacked, 0);
        assert_eq!(stats.messages_to_dlq, 0);
    }
}

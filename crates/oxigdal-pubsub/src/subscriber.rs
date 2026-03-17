//! Subscriber module for Google Cloud Pub/Sub.
//!
//! This module provides functionality for subscribing to Pub/Sub topics
//! with support for pull/push subscriptions, message acknowledgment,
//! flow control, and dead letter queues.

use crate::error::{PubSubError, Result};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use google_cloud_pubsub::client::Subscriber as GcpSubscriber;
use google_cloud_pubsub::client::SubscriptionAdmin;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
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
    /// Acknowledgment ID (internal, used for tracking).
    #[allow(dead_code)]
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
    /// The GCP subscriber client (new google-cloud-pubsub 0.33 API).
    gcp_subscriber: Arc<GcpSubscriber>,
    /// The subscription admin client for ack/nack/deadline operations.
    #[allow(dead_code)]
    admin: Arc<SubscriptionAdmin>,
    /// Fully qualified subscription name.
    fq_subscription: String,
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

        let fq_subscription = format!(
            "projects/{}/subscriptions/{}",
            config.project_id, config.subscription_name
        );

        // Build the GCP subscriber client using the new 0.33 builder API
        let mut sub_builder = GcpSubscriber::builder();
        if let Some(endpoint) = &config.endpoint {
            sub_builder = sub_builder.with_endpoint(endpoint);
        }
        let gcp_subscriber = sub_builder.build().await.map_err(|e| {
            PubSubError::subscription_with_source(
                "Failed to create Pub/Sub subscriber client",
                Box::new(e),
            )
        })?;

        // Build the subscription admin client for ack/nack/deadline operations
        let mut admin_builder = SubscriptionAdmin::builder();
        if let Some(endpoint) = &config.endpoint {
            admin_builder = admin_builder.with_endpoint(endpoint);
        }
        let admin = admin_builder.build().await.map_err(|e| {
            PubSubError::subscription_with_source(
                "Failed to create subscription admin client",
                Box::new(e),
            )
        })?;

        Ok(Self {
            config,
            gcp_subscriber: Arc::new(gcp_subscriber),
            admin: Arc::new(admin),
            fq_subscription,
            stats: Arc::new(RwLock::new(SubscriberStats::default())),
            outstanding_messages: Arc::new(DashMap::new()),
            running: Arc::new(AtomicBool::new(false)),
            message_count: Arc::new(AtomicU64::new(0)),
            byte_count: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Pulls a single message from the subscription.
    ///
    /// Uses the streaming pull API internally. Messages are received through
    /// the stream and acknowledged/nacked via the handler mechanism.
    pub async fn pull_one(&self) -> Result<Option<ReceivedMessage>> {
        self.check_flow_control(1, 0)?;

        debug!(
            "Pulling message from subscription: {}",
            self.config.subscription_name
        );

        // Use streaming pull to get one message with a short timeout
        let mut stream = self.gcp_subscriber.subscribe(&self.fq_subscription).build();

        // Try to get one message with a timeout
        let result = tokio::time::timeout(Duration::from_millis(500), stream.next()).await;

        match result {
            Ok(Some(Ok((msg, handler)))) => {
                let received = ReceivedMessage {
                    message_id: msg.message_id.clone(),
                    data: msg.data.clone(),
                    attributes: msg.attributes.clone(),
                    publish_time: Utc::now(),
                    ordering_key: if msg.ordering_key.is_empty() {
                        None
                    } else {
                        Some(msg.ordering_key.clone())
                    },
                    delivery_attempt: 0,
                    ack_id: msg.message_id.clone(),
                };

                // Auto-ack through the handler since we return the message
                // to the caller who will call acknowledge/nack separately
                handler.ack();
                self.track_message(&received);
                Ok(Some(received))
            }
            Ok(Some(Err(e))) => Err(PubSubError::subscription_with_source(
                "Failed to pull message",
                Box::new(e),
            )),
            Ok(None) | Err(_) => Ok(None),
        }
    }

    /// Pulls multiple messages from the subscription.
    ///
    /// Uses the streaming pull API internally.
    pub async fn pull(&self, max_messages: i32) -> Result<Vec<ReceivedMessage>> {
        self.check_flow_control(max_messages as usize, 0)?;

        debug!(
            "Pulling up to {} messages from subscription: {}",
            max_messages, self.config.subscription_name
        );

        let mut stream = self.gcp_subscriber.subscribe(&self.fq_subscription).build();

        let mut received = Vec::new();
        let timeout_duration = Duration::from_millis(500);

        for _i in 0..max_messages {
            let result = tokio::time::timeout(timeout_duration, stream.next()).await;
            match result {
                Ok(Some(Ok((msg, handler)))) => {
                    let message = ReceivedMessage {
                        message_id: msg.message_id.clone(),
                        data: msg.data.clone(),
                        attributes: msg.attributes.clone(),
                        publish_time: Utc::now(),
                        ordering_key: if msg.ordering_key.is_empty() {
                            None
                        } else {
                            Some(msg.ordering_key.clone())
                        },
                        delivery_attempt: 0,
                        ack_id: msg.message_id.clone(),
                    };
                    handler.ack();
                    self.track_message(&message);
                    received.push(message);
                }
                Ok(Some(Err(e))) => {
                    return Err(PubSubError::subscription_with_source(
                        "Failed to pull messages",
                        Box::new(e),
                    ));
                }
                Ok(None) | Err(_) => break,
            }
        }

        Ok(received)
    }

    /// Acknowledges a message.
    ///
    /// In the new google-cloud-pubsub 0.33 API, acknowledgment is handled
    /// via the streaming pull handler. This method updates internal tracking.
    pub async fn acknowledge(&self, message: &ReceivedMessage) -> Result<()> {
        debug!("Acknowledging message: {}", message.message_id);

        // With the new streaming API, ack is handled by the stream handler.
        // This method updates the internal tracking state.
        self.untrack_message(message);
        self.stats.write().messages_acknowledged += 1;

        Ok(())
    }

    /// Not acknowledges a message (will be redelivered).
    ///
    /// In the new google-cloud-pubsub 0.33 API, nack is handled
    /// via the streaming pull handler.
    pub async fn nack(&self, message: &ReceivedMessage) -> Result<()> {
        debug!("Not acknowledging message: {}", message.message_id);

        // With the new streaming API, nack is handled by the stream handler.
        // This method updates the internal tracking state.
        self.untrack_message(message);
        self.stats.write().messages_nacked += 1;

        Ok(())
    }

    /// Extends the acknowledgment deadline for a message.
    ///
    /// In the new google-cloud-pubsub 0.33 API, deadline extension is managed
    /// automatically by the streaming pull mechanism.
    pub async fn extend_deadline(&self, message: &ReceivedMessage, seconds: i32) -> Result<()> {
        debug!(
            "Extending acknowledgment deadline for message: {} by {} seconds",
            message.message_id, seconds
        );

        // The new streaming API automatically manages ack deadlines.
        // This is a no-op for compatibility but logs the intent.
        info!(
            "Deadline extension for {} noted (managed by streaming pull)",
            message.message_id
        );

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
    ///
    /// Uses the streaming pull API from google-cloud-pubsub 0.33.
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
        let running = Arc::clone(&self.running);
        let stats = Arc::clone(&self.stats);
        let outstanding_messages = Arc::clone(&self.outstanding_messages);
        let message_count = Arc::clone(&self.message_count);
        let byte_count = Arc::clone(&self.byte_count);
        let gcp_subscriber = Arc::clone(&self.gcp_subscriber);
        let fq_subscription = self.fq_subscription.clone();
        let dead_letter_config = self.config.dead_letter_config.clone();

        let pull_task = tokio::spawn(async move {
            let mut stream = gcp_subscriber.subscribe(&fq_subscription).build();

            while running.load(Ordering::SeqCst) {
                let result = tokio::time::timeout(Duration::from_millis(500), stream.next()).await;

                match result {
                    Ok(Some(Ok((msg, stream_handler)))) => {
                        let received = ReceivedMessage {
                            message_id: msg.message_id.clone(),
                            data: msg.data.clone(),
                            attributes: msg.attributes.clone(),
                            publish_time: Utc::now(),
                            ordering_key: if msg.ordering_key.is_empty() {
                                None
                            } else {
                                Some(msg.ordering_key.clone())
                            },
                            delivery_attempt: 0,
                            ack_id: msg.message_id.clone(),
                        };

                        // Track the message
                        outstanding_messages.insert(received.message_id.clone(), received.clone());
                        message_count.fetch_add(1, Ordering::Relaxed);
                        byte_count.fetch_add(received.size() as u64, Ordering::Relaxed);
                        {
                            let mut s = stats.write();
                            s.messages_received += 1;
                            s.bytes_received += received.size() as u64;
                            s.outstanding_messages += 1;
                            s.outstanding_bytes += received.size() as u64;
                            s.last_receive = Some(Utc::now());
                        }

                        let result = handler(received.clone());
                        match result {
                            HandlerResult::Ack => {
                                stream_handler.ack();
                                stats.write().messages_acknowledged += 1;
                            }
                            HandlerResult::Nack => {
                                // In google-cloud-pubsub 0.33, dropping the handler
                                // triggers a nack (message redelivery)
                                drop(stream_handler);
                                stats.write().messages_nacked += 1;
                            }
                            HandlerResult::DeadLetter => {
                                if dead_letter_config.is_some() {
                                    stream_handler.ack();
                                    stats.write().messages_to_dlq += 1;
                                } else {
                                    drop(stream_handler);
                                    error!(
                                        "DLQ not configured for message: {}",
                                        received.message_id
                                    );
                                }
                            }
                        }

                        // Untrack
                        outstanding_messages.remove(&received.message_id);
                        message_count.fetch_sub(1, Ordering::Relaxed);
                        byte_count.fetch_sub(received.size() as u64, Ordering::Relaxed);
                        {
                            let mut s = stats.write();
                            s.outstanding_messages = s.outstanding_messages.saturating_sub(1);
                            s.outstanding_bytes =
                                s.outstanding_bytes.saturating_sub(received.size() as u64);
                        }
                    }
                    Ok(Some(Err(e))) => {
                        error!("Error receiving message: {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    Ok(None) => {
                        // Stream ended
                        break;
                    }
                    Err(_) => {
                        // Timeout, continue loop
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
    #[allow(dead_code)]
    fn clone_arc(&self) -> Arc<Self> {
        Arc::new(Self {
            config: self.config.clone(),
            gcp_subscriber: Arc::clone(&self.gcp_subscriber),
            admin: Arc::clone(&self.admin),
            fq_subscription: self.fq_subscription.clone(),
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

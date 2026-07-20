//! MQTT subscriber implementation

mod handler;
mod router;

pub use handler::{MessageCallback, SimpleHandler};
pub use router::{RouterConfig, TopicRouter};

use crate::client::{MessageHandler, MqttClient};
use crate::error::{MqttError, Result, SubscriptionError};
use crate::types::{Message, QoS, TopicFilter};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::debug;

/// Subscriber configuration
#[derive(Debug, Clone)]
pub struct SubscriberConfig {
    /// Default QoS level for subscriptions
    pub default_qos: QoS,
    /// Message buffer size
    pub buffer_size: usize,
    /// Enable auto-acknowledgment
    pub auto_ack: bool,
    /// Maximum concurrent handlers
    pub max_concurrent_handlers: usize,
}

impl Default for SubscriberConfig {
    fn default() -> Self {
        Self {
            default_qos: QoS::AtMostOnce,
            buffer_size: 100,
            auto_ack: true,
            max_concurrent_handlers: 10,
        }
    }
}

impl SubscriberConfig {
    /// Create new subscriber configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set default QoS
    pub fn with_qos(mut self, qos: QoS) -> Self {
        self.default_qos = qos;
        self
    }

    /// Set buffer size
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Set auto-acknowledgment
    pub fn with_auto_ack(mut self, enable: bool) -> Self {
        self.auto_ack = enable;
        self
    }

    /// Set maximum concurrent handlers
    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent_handlers = max;
        self
    }
}

/// MQTT subscriber
pub struct Subscriber {
    /// MQTT client
    client: Arc<MqttClient>,
    /// Configuration
    config: SubscriberConfig,
}

impl Subscriber {
    /// Create a new subscriber
    pub fn new(client: Arc<MqttClient>, config: SubscriberConfig) -> Self {
        Self { client, config }
    }

    /// Subscribe to a topic with a handler
    pub async fn subscribe<H>(&self, filter: TopicFilter, handler: H) -> Result<()>
    where
        H: MessageHandler + 'static,
    {
        let handler = Arc::new(handler);
        self.client.subscribe(filter, handler).await
    }

    /// Subscribe to a topic with a callback function
    pub async fn subscribe_callback<F>(&self, filter: TopicFilter, callback: F) -> Result<()>
    where
        F: Fn(Message) -> Result<()> + Send + Sync + 'static,
    {
        let handler = SimpleHandler::new(callback);
        self.subscribe(filter, handler).await
    }

    /// Subscribe to a topic with an async callback
    pub async fn subscribe_async<F, Fut>(&self, filter: TopicFilter, callback: F) -> Result<()>
    where
        F: Fn(Message) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        let handler = AsyncCallbackHandler { callback };
        self.subscribe(filter, handler).await
    }

    /// Subscribe to a topic and receive messages via channel
    pub async fn subscribe_channel(&self, filter: TopicFilter) -> Result<mpsc::Receiver<Message>> {
        let (tx, rx) = mpsc::channel(self.config.buffer_size);
        let handler = ChannelHandler { tx };
        self.subscribe(filter, handler).await?;
        Ok(rx)
    }

    /// Subscribe to multiple topics
    pub async fn subscribe_many<H>(&self, subscriptions: Vec<(TopicFilter, H)>) -> Result<()>
    where
        H: MessageHandler + Clone + 'static,
    {
        for (filter, handler) in subscriptions {
            self.subscribe(filter, handler.clone()).await?;
        }
        Ok(())
    }

    /// Unsubscribe from a topic
    pub async fn unsubscribe(&self, topic: &str) -> Result<()> {
        self.client.unsubscribe(topic).await
    }

    /// Unsubscribe from multiple topics
    pub async fn unsubscribe_many(&self, topics: Vec<&str>) -> Result<()> {
        for topic in topics {
            self.unsubscribe(topic).await?;
        }
        Ok(())
    }

    /// Get list of active subscriptions
    pub async fn subscriptions(&self) -> Vec<String> {
        self.client.subscriptions().await
    }

    /// Check if subscribed to a topic
    pub async fn is_subscribed(&self, topic: &str) -> bool {
        self.subscriptions().await.contains(&topic.to_string())
    }

    /// Get configuration
    pub fn config(&self) -> &SubscriberConfig {
        &self.config
    }
}

/// Channel-based message handler
struct ChannelHandler {
    /// Message sender
    tx: mpsc::Sender<Message>,
}

#[async_trait]
impl MessageHandler for ChannelHandler {
    async fn handle_message(&self, message: Message) -> Result<()> {
        self.tx
            .send(message)
            .await
            .map_err(|e| MqttError::Internal(format!("Failed to send message to channel: {}", e)))
    }
}

/// Async callback handler
struct AsyncCallbackHandler<F, Fut>
where
    F: Fn(Message) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<()>> + Send,
{
    /// Callback function
    callback: F,
}

#[async_trait]
impl<F, Fut> MessageHandler for AsyncCallbackHandler<F, Fut>
where
    F: Fn(Message) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<()>> + Send + 'static,
{
    async fn handle_message(&self, message: Message) -> Result<()> {
        (self.callback)(message).await
    }
}

/// Topic subscription manager
pub struct TopicSubscription {
    /// Subscriber
    subscriber: Arc<Subscriber>,
    /// Topic filter
    filter: TopicFilter,
    /// Is subscribed
    subscribed: Arc<std::sync::atomic::AtomicBool>,
}

impl TopicSubscription {
    /// Create a new topic subscription
    pub fn new(subscriber: Arc<Subscriber>, filter: TopicFilter) -> Self {
        Self {
            subscriber,
            filter,
            subscribed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Subscribe with a handler
    pub async fn subscribe<H>(&self, handler: H) -> Result<()>
    where
        H: MessageHandler + 'static,
    {
        if self.is_subscribed() {
            return Err(MqttError::Subscription(
                SubscriptionError::SubscribeFailed {
                    topic: self.filter.pattern.clone(),
                    reason: "Already subscribed".to_string(),
                },
            ));
        }

        self.subscriber
            .subscribe(self.filter.clone(), handler)
            .await?;
        self.subscribed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    /// Unsubscribe
    pub async fn unsubscribe(&self) -> Result<()> {
        if !self.is_subscribed() {
            return Ok(());
        }

        self.subscriber.unsubscribe(&self.filter.pattern).await?;
        self.subscribed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    /// Check if subscribed
    pub fn is_subscribed(&self) -> bool {
        self.subscribed.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Get topic filter
    pub fn filter(&self) -> &TopicFilter {
        &self.filter
    }
}

impl Drop for TopicSubscription {
    fn drop(&mut self) {
        if self.is_subscribed() {
            // Note: We can't await in Drop, so we just mark as unsubscribed
            // The actual unsubscribe should be called explicitly
            debug!("TopicSubscription dropped for: {}", self.filter.pattern);
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::client::ClientConfig;
    use crate::types::ConnectionOptions;

    #[tokio::test]
    async fn test_subscriber_creation() {
        let conn_opts = ConnectionOptions::new("localhost", 1883, "test-sub");
        let client_config = ClientConfig::new(conn_opts);
        let client = MqttClient::new(client_config).expect("Failed to create client");
        let client = Arc::new(client);

        let sub_config = SubscriberConfig::new();
        let subscriber = Subscriber::new(client, sub_config);

        assert_eq!(subscriber.config().default_qos, QoS::AtMostOnce);
        assert_eq!(subscriber.config().buffer_size, 100);
    }

    #[test]
    fn test_subscriber_config() {
        let config = SubscriberConfig::new()
            .with_qos(QoS::ExactlyOnce)
            .with_buffer_size(200)
            .with_auto_ack(false)
            .with_max_concurrent(20);

        assert_eq!(config.default_qos, QoS::ExactlyOnce);
        assert_eq!(config.buffer_size, 200);
        assert!(!config.auto_ack);
        assert_eq!(config.max_concurrent_handlers, 20);
    }

    #[test]
    fn test_topic_filter_creation() {
        let filter = TopicFilter::new("sensor/+/temperature", QoS::AtLeastOnce);
        assert_eq!(filter.pattern, "sensor/+/temperature");
        assert_eq!(filter.qos, QoS::AtLeastOnce);
    }
}

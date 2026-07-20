//! Pub/sub channels for topic-based messaging

use crate::error::{Error, Result};
use crate::protocol::message::Message;
use crate::server::connection::ConnectionId;
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;

/// Channel configuration
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Maximum subscribers
    pub max_subscribers: usize,
    /// Buffer size for broadcast channel
    pub buffer_size: usize,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            max_subscribers: 10_000,
            buffer_size: 1000,
        }
    }
}

/// Generic channel interface
pub trait Channel: Send + Sync {
    /// Subscribe to the channel
    fn subscribe(
        &self,
        subscriber: ConnectionId,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Unsubscribe from the channel
    fn unsubscribe(
        &self,
        subscriber: &ConnectionId,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Publish a message to the channel
    fn publish(&self, message: Message) -> impl std::future::Future<Output = Result<usize>> + Send;

    /// Get subscriber count
    fn subscriber_count(&self) -> impl std::future::Future<Output = usize> + Send;
}

/// Topic-based channel
pub struct TopicChannel {
    topic: String,
    config: ChannelConfig,
    subscribers: Arc<DashMap<ConnectionId, broadcast::Sender<Message>>>,
    tx: broadcast::Sender<Message>,
    stats: Arc<ChannelStatistics>,
}

/// Channel statistics
struct ChannelStatistics {
    messages_published: AtomicU64,
    messages_delivered: AtomicU64,
    messages_dropped: AtomicU64,
}

impl TopicChannel {
    /// Create a new topic channel
    pub fn new(topic: String, config: ChannelConfig) -> Self {
        let (tx, _) = broadcast::channel(config.buffer_size);

        Self {
            topic,
            config,
            subscribers: Arc::new(DashMap::new()),
            tx,
            stats: Arc::new(ChannelStatistics {
                messages_published: AtomicU64::new(0),
                messages_delivered: AtomicU64::new(0),
                messages_dropped: AtomicU64::new(0),
            }),
        }
    }

    /// Get topic name
    pub fn topic(&self) -> &str {
        &self.topic
    }

    /// Get statistics
    pub async fn stats(&self) -> ChannelStats {
        ChannelStats {
            topic: self.topic.clone(),
            subscriber_count: self.subscribers.len(),
            messages_published: self.stats.messages_published.load(Ordering::Relaxed),
            messages_delivered: self.stats.messages_delivered.load(Ordering::Relaxed),
            messages_dropped: self.stats.messages_dropped.load(Ordering::Relaxed),
        }
    }
}

impl Channel for TopicChannel {
    async fn subscribe(&self, subscriber: ConnectionId) -> Result<()> {
        if self.subscribers.len() >= self.config.max_subscribers {
            return Err(Error::ResourceExhausted(format!(
                "Topic {} has reached maximum subscribers ({})",
                self.topic, self.config.max_subscribers
            )));
        }

        self.subscribers.insert(subscriber, self.tx.clone());
        tracing::debug!("Subscriber {} joined topic {}", subscriber, self.topic);
        Ok(())
    }

    async fn unsubscribe(&self, subscriber: &ConnectionId) -> Result<()> {
        self.subscribers.remove(subscriber);
        tracing::debug!("Subscriber {} left topic {}", subscriber, self.topic);
        Ok(())
    }

    async fn publish(&self, message: Message) -> Result<usize> {
        self.stats
            .messages_published
            .fetch_add(1, Ordering::Relaxed);

        match self.tx.send(message) {
            Ok(count) => {
                self.stats
                    .messages_delivered
                    .fetch_add(count as u64, Ordering::Relaxed);
                Ok(count)
            }
            Err(_) => {
                self.stats.messages_dropped.fetch_add(1, Ordering::Relaxed);
                Ok(0)
            }
        }
    }

    async fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }
}

/// Channel statistics snapshot
#[derive(Debug, Clone)]
pub struct ChannelStats {
    /// Topic name
    pub topic: String,
    /// Number of subscribers
    pub subscriber_count: usize,
    /// Messages published
    pub messages_published: u64,
    /// Messages delivered
    pub messages_delivered: u64,
    /// Messages dropped
    pub messages_dropped: u64,
}

/// Multi-topic channel manager
pub struct MultiChannelManager {
    channels: Arc<DashMap<String, Arc<TopicChannel>>>,
    default_config: ChannelConfig,
}

impl MultiChannelManager {
    /// Create a new multi-channel manager
    pub fn new(default_config: ChannelConfig) -> Self {
        Self {
            channels: Arc::new(DashMap::new()),
            default_config,
        }
    }

    /// Get or create a channel
    pub fn get_or_create(&self, topic: &str) -> Arc<TopicChannel> {
        self.channels
            .entry(topic.to_string())
            .or_insert_with(|| {
                Arc::new(TopicChannel::new(
                    topic.to_string(),
                    self.default_config.clone(),
                ))
            })
            .clone()
    }

    /// Subscribe to a topic
    pub async fn subscribe(&self, topic: &str, subscriber: ConnectionId) -> Result<()> {
        let channel = self.get_or_create(topic);
        channel.subscribe(subscriber).await
    }

    /// Unsubscribe from a topic
    pub async fn unsubscribe(&self, topic: &str, subscriber: &ConnectionId) -> Result<()> {
        if let Some(channel) = self.channels.get(topic) {
            channel.unsubscribe(subscriber).await?;
        }
        Ok(())
    }

    /// Publish to a topic
    pub async fn publish(&self, topic: &str, message: Message) -> Result<usize> {
        if let Some(channel) = self.channels.get(topic) {
            channel.publish(message).await
        } else {
            Ok(0)
        }
    }

    /// Get all topics
    pub fn topics(&self) -> Vec<String> {
        self.channels.iter().map(|r| r.key().clone()).collect()
    }

    /// Get channel count
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Remove a channel
    pub fn remove_channel(&self, topic: &str) -> Option<Arc<TopicChannel>> {
        self.channels.remove(topic).map(|(_, v)| v)
    }

    /// Clear all channels
    pub fn clear(&self) {
        self.channels.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_topic_channel() {
        let config = ChannelConfig::default();
        let channel = TopicChannel::new("test".to_string(), config);

        assert_eq!(channel.topic(), "test");
        assert_eq!(channel.subscriber_count().await, 0);
    }

    #[tokio::test]
    async fn test_channel_subscribe() -> Result<()> {
        let config = ChannelConfig::default();
        let channel = TopicChannel::new("test".to_string(), config);

        let subscriber = ConnectionId::new_v4();
        channel.subscribe(subscriber).await?;

        assert_eq!(channel.subscriber_count().await, 1);
        Ok(())
    }

    #[tokio::test]
    async fn test_channel_unsubscribe() -> Result<()> {
        let config = ChannelConfig::default();
        let channel = TopicChannel::new("test".to_string(), config);

        let subscriber = ConnectionId::new_v4();
        channel.subscribe(subscriber).await?;
        channel.unsubscribe(&subscriber).await?;

        assert_eq!(channel.subscriber_count().await, 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_channel_max_subscribers() {
        let config = ChannelConfig {
            max_subscribers: 2,
            buffer_size: 10,
        };
        let channel = TopicChannel::new("test".to_string(), config);

        let sub1 = ConnectionId::new_v4();
        let sub2 = ConnectionId::new_v4();
        let sub3 = ConnectionId::new_v4();

        assert!(channel.subscribe(sub1).await.is_ok());
        assert!(channel.subscribe(sub2).await.is_ok());
        assert!(channel.subscribe(sub3).await.is_err());
    }

    #[tokio::test]
    async fn test_multi_channel_manager() {
        let config = ChannelConfig::default();
        let manager = MultiChannelManager::new(config);

        assert_eq!(manager.channel_count(), 0);

        let channel = manager.get_or_create("test");
        assert_eq!(manager.channel_count(), 1);
        assert_eq!(channel.topic(), "test");
    }

    #[tokio::test]
    async fn test_multi_channel_publish() -> Result<()> {
        let config = ChannelConfig::default();
        let manager = MultiChannelManager::new(config);

        let subscriber = ConnectionId::new_v4();
        manager.subscribe("test", subscriber).await?;

        // Get the channel to keep a receiver alive
        let channel = manager.get_or_create("test");
        let mut _rx = channel.tx.subscribe();

        let message = Message::ping();
        let count = manager.publish("test", message).await?;

        // Should deliver to 1 receiver
        assert_eq!(count, 1);
        Ok(())
    }
}

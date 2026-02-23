//! Broadcasting and pub/sub system
//!
//! This module provides:
//! - Pub/sub channels for topic-based messaging
//! - Room management for group communications
//! - Selective broadcasting with message filters
//! - Message routing and distribution

pub mod channel;
pub mod filter;
pub mod room;
pub mod router;

pub use channel::{Channel, ChannelConfig, ChannelStats, TopicChannel};
pub use filter::{FilterChain, FilterPredicate, MessageFilter};
pub use room::{Room, RoomManager, RoomStats};
pub use router::{MessageRouter, RoutingRule, RoutingStrategy};

use crate::error::Result;
use crate::protocol::message::Message;
use crate::server::connection::ConnectionId;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Broadcast configuration
#[derive(Debug, Clone)]
pub struct BroadcastConfig {
    /// Maximum subscribers per topic
    pub max_subscribers_per_topic: usize,
    /// Maximum topics
    pub max_topics: usize,
    /// Maximum rooms
    pub max_rooms: usize,
    /// Maximum members per room
    pub max_members_per_room: usize,
    /// Enable message filtering
    pub enable_filtering: bool,
    /// Channel buffer size
    pub channel_buffer_size: usize,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        Self {
            max_subscribers_per_topic: 10_000,
            max_topics: 1_000,
            max_rooms: 1_000,
            max_members_per_room: 10_000,
            enable_filtering: true,
            channel_buffer_size: 1000,
        }
    }
}

/// Broadcast system
pub struct BroadcastSystem {
    config: BroadcastConfig,
    channels: Arc<RwLock<std::collections::HashMap<String, Arc<TopicChannel>>>>,
    room_manager: Arc<RoomManager>,
    router: Arc<MessageRouter>,
}

impl BroadcastSystem {
    /// Create a new broadcast system
    pub fn new(config: BroadcastConfig) -> Self {
        Self {
            config: config.clone(),
            channels: Arc::new(RwLock::new(std::collections::HashMap::new())),
            room_manager: Arc::new(RoomManager::new(
                config.max_rooms,
                config.max_members_per_room,
            )),
            router: Arc::new(MessageRouter::new()),
        }
    }

    /// Subscribe to a topic
    pub async fn subscribe(&self, topic: String, subscriber: ConnectionId) -> Result<()> {
        let mut channels = self.channels.write().await;

        let channel = channels.entry(topic.clone()).or_insert_with(|| {
            Arc::new(TopicChannel::new(
                topic.clone(),
                ChannelConfig {
                    max_subscribers: self.config.max_subscribers_per_topic,
                    buffer_size: self.config.channel_buffer_size,
                },
            ))
        });

        channel.subscribe(subscriber).await
    }

    /// Unsubscribe from a topic
    pub async fn unsubscribe(&self, topic: &str, subscriber: &ConnectionId) -> Result<()> {
        let channels = self.channels.read().await;

        if let Some(channel) = channels.get(topic) {
            channel.unsubscribe(subscriber).await?;
        }

        Ok(())
    }

    /// Publish a message to a topic
    pub async fn publish(&self, topic: &str, message: Message) -> Result<usize> {
        let channels = self.channels.read().await;

        if let Some(channel) = channels.get(topic) {
            channel.publish(message).await
        } else {
            Ok(0)
        }
    }

    /// Get room manager
    pub fn room_manager(&self) -> &Arc<RoomManager> {
        &self.room_manager
    }

    /// Get message router
    pub fn router(&self) -> &Arc<MessageRouter> {
        &self.router
    }

    /// Get broadcast statistics
    pub async fn stats(&self) -> BroadcastStats {
        let channels = self.channels.read().await;
        let mut total_subscribers = 0;
        let mut total_messages = 0;

        for channel in channels.values() {
            let stats = channel.stats().await;
            total_subscribers += stats.subscriber_count;
            total_messages += stats.messages_published;
        }

        let room_stats = self.room_manager.stats().await;

        BroadcastStats {
            topic_count: channels.len(),
            total_subscribers,
            total_messages,
            room_count: room_stats.total_rooms,
            total_room_members: room_stats.total_members,
        }
    }
}

/// Broadcast statistics
#[derive(Debug, Clone)]
pub struct BroadcastStats {
    /// Number of topics
    pub topic_count: usize,
    /// Total subscribers across all topics
    pub total_subscribers: usize,
    /// Total messages published
    pub total_messages: u64,
    /// Number of rooms
    pub room_count: usize,
    /// Total room members
    pub total_room_members: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broadcast_config_default() {
        let config = BroadcastConfig::default();
        assert!(config.enable_filtering);
        assert_eq!(config.max_topics, 1_000);
    }

    #[tokio::test]
    async fn test_broadcast_system() {
        let config = BroadcastConfig::default();
        let system = BroadcastSystem::new(config);

        let stats = system.stats().await;
        assert_eq!(stats.topic_count, 0);
        assert_eq!(stats.total_subscribers, 0);
    }
}

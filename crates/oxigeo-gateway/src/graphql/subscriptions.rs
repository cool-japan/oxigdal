//! GraphQL subscription implementations.

use futures::Stream;
use std::pin::Pin;

/// Subscription manager.
pub struct SubscriptionManager {
    subscribers: dashmap::DashMap<String, Vec<String>>,
}

impl SubscriptionManager {
    /// Creates a new subscription manager.
    pub fn new() -> Self {
        Self {
            subscribers: dashmap::DashMap::new(),
        }
    }

    /// Subscribes to a topic.
    pub fn subscribe(&self, topic: String, subscriber_id: String) {
        self.subscribers
            .entry(topic)
            .or_default()
            .push(subscriber_id);
    }

    /// Unsubscribes from a topic.
    pub fn unsubscribe(&self, topic: &str, subscriber_id: &str) {
        if let Some(mut subs) = self.subscribers.get_mut(topic) {
            subs.retain(|id| id != subscriber_id);
        }
    }

    /// Gets all subscribers for a topic.
    pub fn get_subscribers(&self, topic: &str) -> Vec<String> {
        self.subscribers
            .get(topic)
            .map(|subs| subs.clone())
            .unwrap_or_default()
    }

    /// Publishes a message to all subscribers of a topic.
    pub async fn publish(&self, topic: &str, _message: Vec<u8>) -> usize {
        let subscribers = self.get_subscribers(topic);
        let count = subscribers.len();

        // In a real implementation, send message to all subscribers
        for _subscriber_id in subscribers {
            // Send message
        }

        count
    }
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Creates a stream for dataset changes.
pub fn dataset_change_stream(dataset_id: String) -> Pin<Box<dyn Stream<Item = String> + Send>> {
    Box::pin(async_stream::stream! {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(10));
        loop {
            interval.tick().await;
            yield format!("Dataset {} changed", dataset_id);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_manager() {
        let manager = SubscriptionManager::new();

        manager.subscribe("topic1".to_string(), "sub1".to_string());
        manager.subscribe("topic1".to_string(), "sub2".to_string());

        let subs = manager.get_subscribers("topic1");
        assert_eq!(subs.len(), 2);

        manager.unsubscribe("topic1", "sub1");
        let subs = manager.get_subscribers("topic1");
        assert_eq!(subs.len(), 1);
    }
}

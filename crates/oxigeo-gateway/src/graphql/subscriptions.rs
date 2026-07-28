//! GraphQL subscription implementations.

use futures::Stream;
use std::pin::Pin;
use tokio::sync::mpsc;

/// A single subscriber: its id and the channel used to push messages to it.
struct Subscriber {
    id: String,
    sender: mpsc::UnboundedSender<Vec<u8>>,
}

/// Subscription manager.
pub struct SubscriptionManager {
    subscribers: dashmap::DashMap<String, Vec<Subscriber>>,
}

impl SubscriptionManager {
    /// Creates a new subscription manager.
    pub fn new() -> Self {
        Self {
            subscribers: dashmap::DashMap::new(),
        }
    }

    /// Subscribes to a topic, returning the receiver on which published messages arrive.
    ///
    /// The returned [`mpsc::UnboundedReceiver`] yields every message subsequently published
    /// to `topic` until the subscriber is removed (via [`Self::unsubscribe`]) or the receiver
    /// is dropped.
    #[must_use]
    pub fn subscribe(
        &self,
        topic: String,
        subscriber_id: String,
    ) -> mpsc::UnboundedReceiver<Vec<u8>> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut entry = self.subscribers.entry(topic).or_default();
        // Replace any existing registration for the same id so re-subscribing is idempotent.
        entry.retain(|s| s.id != subscriber_id);
        entry.push(Subscriber {
            id: subscriber_id,
            sender,
        });
        receiver
    }

    /// Unsubscribes from a topic.
    pub fn unsubscribe(&self, topic: &str, subscriber_id: &str) {
        if let Some(mut subs) = self.subscribers.get_mut(topic) {
            subs.retain(|s| s.id != subscriber_id);
        }
    }

    /// Gets all subscriber ids for a topic.
    pub fn get_subscribers(&self, topic: &str) -> Vec<String> {
        self.subscribers
            .get(topic)
            .map(|subs| subs.iter().map(|s| s.id.clone()).collect())
            .unwrap_or_default()
    }

    /// Publishes a message to all subscribers of a topic.
    ///
    /// The message is sent on each subscriber's channel. Subscribers whose receiver has been
    /// dropped are pruned, and the number of subscribers that actually received the message
    /// is returned.
    pub async fn publish(&self, topic: &str, message: Vec<u8>) -> usize {
        let mut delivered = 0;
        if let Some(mut subs) = self.subscribers.get_mut(topic) {
            // Send to each live subscriber; retain only those whose channel is still open.
            subs.retain(|subscriber| match subscriber.sender.send(message.clone()) {
                Ok(()) => {
                    delivered += 1;
                    true
                }
                Err(_) => false, // receiver dropped -> prune
            });
        }
        delivered
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

    #[tokio::test]
    async fn test_subscription_manager() {
        let manager = SubscriptionManager::new();

        let _rx1 = manager.subscribe("topic1".to_string(), "sub1".to_string());
        let _rx2 = manager.subscribe("topic1".to_string(), "sub2".to_string());

        let subs = manager.get_subscribers("topic1");
        assert_eq!(subs.len(), 2);

        manager.unsubscribe("topic1", "sub1");
        let subs = manager.get_subscribers("topic1");
        assert_eq!(subs.len(), 1);
    }

    #[tokio::test]
    async fn test_publish_delivers_to_subscribers() {
        let manager = SubscriptionManager::new();

        let mut rx1 = manager.subscribe("t".to_string(), "a".to_string());
        let mut rx2 = manager.subscribe("t".to_string(), "b".to_string());

        let delivered = manager.publish("t", b"hello".to_vec()).await;
        assert_eq!(delivered, 2, "both subscribers must receive the message");

        assert_eq!(rx1.recv().await, Some(b"hello".to_vec()));
        assert_eq!(rx2.recv().await, Some(b"hello".to_vec()));
    }

    #[tokio::test]
    async fn test_publish_prunes_dropped_subscribers() {
        let manager = SubscriptionManager::new();

        let rx1 = manager.subscribe("t".to_string(), "a".to_string());
        let _rx2 = manager.subscribe("t".to_string(), "b".to_string());

        // Drop the first subscriber's receiver; publish should deliver to only one and prune.
        drop(rx1);
        let delivered = manager.publish("t", b"x".to_vec()).await;
        assert_eq!(delivered, 1);
        assert_eq!(manager.get_subscribers("t").len(), 1);
    }
}

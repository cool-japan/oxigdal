//! Message batching for improved throughput

use bytes::Bytes;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A batch of messages for a specific topic-partition
#[derive(Debug)]
pub struct MessageBatch {
    /// Topic name
    pub topic: String,
    /// Partition number
    pub partition: i32,
    /// Messages in the batch (key, value)
    pub messages: Vec<(Option<Bytes>, Bytes)>,
    /// Total size in bytes
    pub total_size: usize,
    /// Batch creation time
    pub created_at: Instant,
}

impl MessageBatch {
    /// Create a new message batch
    pub fn new(topic: String, partition: i32) -> Self {
        Self {
            topic,
            partition,
            messages: Vec::new(),
            total_size: 0,
            created_at: Instant::now(),
        }
    }

    /// Add a message to the batch
    pub fn add_message(&mut self, key: Option<Bytes>, value: Bytes) {
        let size = key.as_ref().map(|k| k.len()).unwrap_or(0) + value.len();
        self.total_size += size;
        self.messages.push((key, value));
    }

    /// Check if batch is full
    pub fn is_full(&self, max_size: usize, max_messages: usize) -> bool {
        self.total_size >= max_size || self.messages.len() >= max_messages
    }

    /// Check if batch is expired
    pub fn is_expired(&self, linger_duration: Duration) -> bool {
        self.created_at.elapsed() >= linger_duration
    }

    /// Check if batch is empty
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Get number of messages
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Clear the batch
    pub fn clear(&mut self) {
        self.messages.clear();
        self.total_size = 0;
        self.created_at = Instant::now();
    }
}

/// Manager for message batches
pub struct BatchManager {
    /// Batches by topic-partition
    batches: Arc<RwLock<HashMap<String, MessageBatch>>>,
    /// Maximum batch size in bytes
    max_batch_size: usize,
    /// Maximum number of messages per batch
    max_batch_messages: usize,
    /// Linger duration before sending batch
    linger_duration: Duration,
}

impl BatchManager {
    /// Create a new batch manager
    pub fn new(
        max_batch_size: usize,
        max_batch_messages: usize,
        linger_duration: Duration,
    ) -> Self {
        Self {
            batches: Arc::new(RwLock::new(HashMap::new())),
            max_batch_size,
            max_batch_messages,
            linger_duration,
        }
    }

    /// Add a message to the appropriate batch
    pub fn add_message(
        &self,
        topic: &str,
        partition: i32,
        key: Option<Bytes>,
        value: Bytes,
    ) -> Option<MessageBatch> {
        let batch_key = format!("{}-{}", topic, partition);
        let mut batches = self.batches.write();

        let batch = batches
            .entry(batch_key.clone())
            .or_insert_with(|| MessageBatch::new(topic.to_string(), partition));

        batch.add_message(key, value);

        if batch.is_full(self.max_batch_size, self.max_batch_messages) {
            batches.remove(&batch_key)
        } else {
            None
        }
    }

    /// Get all expired batches
    pub fn get_expired_batches(&self) -> Vec<MessageBatch> {
        let mut batches = self.batches.write();
        let mut expired = Vec::new();

        batches.retain(|_key, batch| {
            if batch.is_expired(self.linger_duration) && !batch.is_empty() {
                expired.push(batch.clone());
                false
            } else {
                true
            }
        });

        expired
    }

    /// Flush all batches
    pub fn flush_all(&self) -> Vec<MessageBatch> {
        let mut batches = self.batches.write();
        let all_batches: Vec<MessageBatch> = batches
            .values()
            .filter(|b| !b.is_empty())
            .cloned()
            .collect();
        batches.clear();
        all_batches
    }

    /// Get number of active batches
    pub fn active_batches(&self) -> usize {
        self.batches.read().len()
    }
}

impl Clone for MessageBatch {
    fn clone(&self) -> Self {
        Self {
            topic: self.topic.clone(),
            partition: self.partition,
            messages: self.messages.clone(),
            total_size: self.total_size,
            created_at: self.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_batch() {
        let mut batch = MessageBatch::new("test-topic".to_string(), 0);
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);

        batch.add_message(Some(Bytes::from("key1")), Bytes::from("value1"));
        assert!(!batch.is_empty());
        assert_eq!(batch.len(), 1);

        batch.add_message(None, Bytes::from("value2"));
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn test_batch_full() {
        let mut batch = MessageBatch::new("test-topic".to_string(), 0);

        // Test size limit
        batch.add_message(None, Bytes::from(vec![0u8; 100]));
        assert!(!batch.is_full(200, 10));
        batch.add_message(None, Bytes::from(vec![0u8; 100]));
        assert!(batch.is_full(200, 10));

        // Test message count limit
        let mut batch = MessageBatch::new("test-topic".to_string(), 0);
        for i in 0..5 {
            batch.add_message(None, Bytes::from(vec![i]));
        }
        assert!(batch.is_full(10000, 5));
    }

    #[test]
    fn test_batch_expired() {
        let batch = MessageBatch::new("test-topic".to_string(), 0);
        assert!(!batch.is_expired(Duration::from_secs(1)));

        std::thread::sleep(Duration::from_millis(100));
        assert!(batch.is_expired(Duration::from_millis(50)));
    }

    #[test]
    fn test_batch_manager() {
        let manager = BatchManager::new(1000, 10, Duration::from_millis(100));

        // Add message that doesn't fill batch
        let result = manager.add_message("test-topic", 0, None, Bytes::from("small message"));
        assert!(result.is_none());
        assert_eq!(manager.active_batches(), 1);

        // Add message that fills batch
        let result = manager.add_message("test-topic", 0, None, Bytes::from(vec![0u8; 1000]));
        assert!(result.is_some(), "Expected batch to be returned");
        if let Some(batch) = result {
            assert_eq!(batch.len(), 2);
        }
    }

    #[test]
    fn test_batch_manager_flush() {
        let manager = BatchManager::new(1000, 10, Duration::from_millis(100));

        manager.add_message("topic1", 0, None, Bytes::from("msg1"));
        manager.add_message("topic1", 1, None, Bytes::from("msg2"));
        manager.add_message("topic2", 0, None, Bytes::from("msg3"));

        let batches = manager.flush_all();
        assert_eq!(batches.len(), 3);
        assert_eq!(manager.active_batches(), 0);
    }
}

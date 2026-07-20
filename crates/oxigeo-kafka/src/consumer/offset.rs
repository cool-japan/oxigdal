//! Offset management for Kafka consumer

use parking_lot::RwLock;
use rdkafka::TopicPartitionList;
use std::collections::HashMap;
use std::sync::Arc;

/// Offset commit strategy
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitStrategy {
    /// Manual commit
    Manual,
    /// Auto commit after each message
    AutoPerMessage,
    /// Auto commit after each batch
    AutoPerBatch,
    /// Auto commit at interval
    AutoInterval,
}

/// Offset store for tracking processed offsets
pub trait OffsetStore: Send + Sync {
    /// Store an offset
    fn store(&self, topic: &str, partition: i32, offset: i64);

    /// Get stored offset
    fn get(&self, topic: &str, partition: i32) -> Option<i64>;

    /// Get all stored offsets
    fn get_all(&self) -> HashMap<(String, i32), i64>;

    /// Clear all stored offsets
    fn clear(&self);

    /// Mark offset as committed
    fn mark_committed(&self, topic: &str, partition: i32, offset: i64);

    /// Get uncommitted offsets
    fn get_uncommitted(&self) -> HashMap<(String, i32), i64>;
}

/// In-memory offset store
#[derive(Debug, Default)]
pub struct InMemoryOffsetStore {
    /// Stored offsets (topic, partition) -> offset
    offsets: Arc<RwLock<HashMap<(String, i32), i64>>>,
    /// Committed offsets (topic, partition) -> offset
    committed: Arc<RwLock<HashMap<(String, i32), i64>>>,
}

impl InMemoryOffsetStore {
    /// Create a new in-memory offset store
    pub fn new() -> Self {
        Self::default()
    }
}

impl OffsetStore for InMemoryOffsetStore {
    fn store(&self, topic: &str, partition: i32, offset: i64) {
        let mut offsets = self.offsets.write();
        offsets.insert((topic.to_string(), partition), offset);
    }

    fn get(&self, topic: &str, partition: i32) -> Option<i64> {
        let offsets = self.offsets.read();
        offsets.get(&(topic.to_string(), partition)).copied()
    }

    fn get_all(&self) -> HashMap<(String, i32), i64> {
        self.offsets.read().clone()
    }

    fn clear(&self) {
        self.offsets.write().clear();
    }

    fn mark_committed(&self, topic: &str, partition: i32, offset: i64) {
        let mut committed = self.committed.write();
        committed.insert((topic.to_string(), partition), offset);
    }

    fn get_uncommitted(&self) -> HashMap<(String, i32), i64> {
        let offsets = self.offsets.read();
        let committed = self.committed.read();

        offsets
            .iter()
            .filter_map(|(key, &offset)| {
                let committed_offset = committed.get(key).copied();
                match committed_offset {
                    Some(committed_offset) if offset > committed_offset => {
                        Some((key.clone(), offset))
                    }
                    None => Some((key.clone(), offset)),
                    _ => None,
                }
            })
            .collect()
    }
}

/// Offset manager for coordinating offset tracking and commits
pub struct OffsetManager {
    /// Offset store
    store: Arc<dyn OffsetStore>,
    /// Commit strategy
    strategy: CommitStrategy,
}

impl OffsetManager {
    /// Create a new offset manager
    pub fn new(strategy: CommitStrategy) -> Self {
        Self {
            store: Arc::new(InMemoryOffsetStore::new()),
            strategy,
        }
    }

    /// Create a new offset manager with custom store
    pub fn new_with_store(strategy: CommitStrategy, store: Arc<dyn OffsetStore>) -> Self {
        Self { store, strategy }
    }

    /// Track an offset
    pub fn track_offset(&self, topic: &str, partition: i32, offset: i64) {
        self.store.store(topic, partition, offset);
    }

    /// Get offsets to commit based on strategy
    pub fn get_offsets_to_commit(&self) -> HashMap<(String, i32), i64> {
        match self.strategy {
            CommitStrategy::Manual => self.store.get_uncommitted(),
            CommitStrategy::AutoPerMessage | CommitStrategy::AutoPerBatch => {
                self.store.get_uncommitted()
            }
            CommitStrategy::AutoInterval => self.store.get_uncommitted(),
        }
    }

    /// Mark offsets as committed
    pub fn mark_committed(&self, tpl: &TopicPartitionList) {
        for elem in tpl.elements() {
            if let rdkafka::Offset::Offset(offset) = elem.offset() {
                self.store
                    .mark_committed(elem.topic(), elem.partition(), offset - 1);
            }
        }
    }

    /// Get commit strategy
    pub fn strategy(&self) -> &CommitStrategy {
        &self.strategy
    }

    /// Check if should commit based on strategy
    pub fn should_commit(&self, message_count: usize) -> bool {
        match self.strategy {
            CommitStrategy::Manual => false,
            CommitStrategy::AutoPerMessage => message_count > 0,
            CommitStrategy::AutoPerBatch => message_count > 0,
            CommitStrategy::AutoInterval => false,
        }
    }

    /// Clear all tracked offsets
    pub fn clear(&self) {
        self.store.clear();
    }
}

/// Offset checkpoint for saving/restoring consumer state
#[derive(Debug, Clone)]
pub struct OffsetCheckpoint {
    /// Checkpointed offsets (topic, partition) -> offset
    pub offsets: HashMap<(String, i32), i64>,
    /// Checkpoint timestamp
    pub timestamp: i64,
}

impl OffsetCheckpoint {
    /// Create a new offset checkpoint
    pub fn new(offsets: HashMap<(String, i32), i64>) -> Self {
        Self {
            offsets,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Create checkpoint from offset manager
    pub fn from_manager(manager: &OffsetManager) -> Self {
        let offsets = manager.store.get_all();
        Self::new(offsets)
    }

    /// Restore offsets to manager
    pub fn restore_to_manager(&self, manager: &OffsetManager) {
        for ((topic, partition), offset) in &self.offsets {
            manager.store.store(topic, *partition, *offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_offset_store() {
        let store = InMemoryOffsetStore::new();

        store.store("topic1", 0, 100);
        store.store("topic1", 1, 200);
        store.store("topic2", 0, 300);

        assert_eq!(store.get("topic1", 0), Some(100));
        assert_eq!(store.get("topic1", 1), Some(200));
        assert_eq!(store.get("topic2", 0), Some(300));
        assert_eq!(store.get("topic3", 0), None);

        let all = store.get_all();
        assert_eq!(all.len(), 3);

        store.clear();
        assert_eq!(store.get_all().len(), 0);
    }

    #[test]
    fn test_offset_store_uncommitted() {
        let store = InMemoryOffsetStore::new();

        store.store("topic1", 0, 100);
        store.store("topic1", 1, 200);

        let uncommitted = store.get_uncommitted();
        assert_eq!(uncommitted.len(), 2);

        store.mark_committed("topic1", 0, 100);

        let uncommitted = store.get_uncommitted();
        assert_eq!(uncommitted.len(), 1);
        assert_eq!(uncommitted.get(&("topic1".to_string(), 1)), Some(&200));

        // Store newer offset for topic1:0
        store.store("topic1", 0, 150);

        let uncommitted = store.get_uncommitted();
        assert_eq!(uncommitted.len(), 2);
        assert_eq!(uncommitted.get(&("topic1".to_string(), 0)), Some(&150));
    }

    #[test]
    fn test_offset_manager() {
        let manager = OffsetManager::new(CommitStrategy::Manual);

        manager.track_offset("topic1", 0, 100);
        manager.track_offset("topic1", 1, 200);

        let to_commit = manager.get_offsets_to_commit();
        assert_eq!(to_commit.len(), 2);
        assert_eq!(to_commit.get(&("topic1".to_string(), 0)), Some(&100));
    }

    #[test]
    fn test_offset_manager_should_commit() {
        let manual = OffsetManager::new(CommitStrategy::Manual);
        assert!(!manual.should_commit(1));

        let auto_per_msg = OffsetManager::new(CommitStrategy::AutoPerMessage);
        assert!(auto_per_msg.should_commit(1));
        assert!(!auto_per_msg.should_commit(0));

        let auto_per_batch = OffsetManager::new(CommitStrategy::AutoPerBatch);
        assert!(auto_per_batch.should_commit(10));
        assert!(!auto_per_batch.should_commit(0));
    }

    #[test]
    fn test_offset_checkpoint() {
        let manager = OffsetManager::new(CommitStrategy::Manual);
        manager.track_offset("topic1", 0, 100);
        manager.track_offset("topic1", 1, 200);

        let checkpoint = OffsetCheckpoint::from_manager(&manager);
        assert_eq!(checkpoint.offsets.len(), 2);

        let new_manager = OffsetManager::new(CommitStrategy::Manual);
        checkpoint.restore_to_manager(&new_manager);

        let restored = new_manager.store.get_all();
        assert_eq!(restored.len(), 2);
        assert_eq!(restored.get(&("topic1".to_string(), 0)), Some(&100));
    }
}

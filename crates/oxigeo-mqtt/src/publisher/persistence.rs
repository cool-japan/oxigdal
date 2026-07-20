//! Message persistence for reliable publishing

#[cfg(feature = "persistence")]
use sled::Db;

#[cfg(feature = "persistence")]
use crate::error::{MqttError, PersistenceError};
#[cfg(feature = "persistence")]
use std::path::Path;

use crate::error::Result;
use crate::publisher::Publisher;
use crate::types::Message;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Message persistence interface
pub trait MessagePersistence: Send + Sync {
    /// Store a message
    fn store(&self, id: &str, message: &Message) -> Result<()>;

    /// Retrieve a message
    fn retrieve(&self, id: &str) -> Result<Option<Message>>;

    /// Delete a message
    fn delete(&self, id: &str) -> Result<()>;

    /// List all stored message IDs
    fn list(&self) -> Result<Vec<String>>;

    /// Clear all stored messages
    fn clear(&self) -> Result<()>;

    /// Get count of stored messages
    fn count(&self) -> Result<usize>;
}

/// In-memory persistence (for testing)
// Public API for in-memory message persistence - useful for testing and lightweight scenarios
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct InMemoryPersistence {
    /// Message storage
    storage: Arc<dashmap::DashMap<String, Message>>,
}

// Public API for in-memory persistence creation
#[allow(dead_code)]
impl InMemoryPersistence {
    /// Create new in-memory persistence
    pub fn new() -> Self {
        Self {
            storage: Arc::new(dashmap::DashMap::new()),
        }
    }
}

impl MessagePersistence for InMemoryPersistence {
    fn store(&self, id: &str, message: &Message) -> Result<()> {
        self.storage.insert(id.to_string(), message.clone());
        Ok(())
    }

    fn retrieve(&self, id: &str) -> Result<Option<Message>> {
        Ok(self.storage.get(id).map(|v| v.clone()))
    }

    fn delete(&self, id: &str) -> Result<()> {
        self.storage.remove(id);
        Ok(())
    }

    fn list(&self) -> Result<Vec<String>> {
        Ok(self
            .storage
            .iter()
            .map(|entry| entry.key().clone())
            .collect())
    }

    fn clear(&self) -> Result<()> {
        self.storage.clear();
        Ok(())
    }

    fn count(&self) -> Result<usize> {
        Ok(self.storage.len())
    }
}

/// Sled-based persistence (disk-backed)
#[cfg(feature = "persistence")]
#[allow(dead_code)]
pub struct SledPersistence {
    /// Sled database
    db: Db,
}

#[cfg(feature = "persistence")]
impl SledPersistence {
    /// Create new sled persistence
    #[allow(dead_code)]
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let db = sled::open(path.as_ref()).map_err(|e| {
            MqttError::Persistence(PersistenceError::OpenFailed {
                path: path.as_ref().display().to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(Self { db })
    }

    /// Serialize message
    #[allow(dead_code)]
    fn serialize_message(message: &Message) -> Result<Vec<u8>> {
        serde_json::to_vec(message).map_err(|e| MqttError::Serialization(e.to_string()))
    }

    /// Deserialize message
    #[allow(dead_code)]
    fn deserialize_message(data: &[u8]) -> Result<Message> {
        serde_json::from_slice(data).map_err(|e| MqttError::Serialization(e.to_string()))
    }
}

#[cfg(feature = "persistence")]
impl MessagePersistence for SledPersistence {
    fn store(&self, id: &str, message: &Message) -> Result<()> {
        let data = Self::serialize_message(message)?;
        self.db
            .insert(id, data)
            .map_err(|e| MqttError::Persistence(PersistenceError::WriteFailed(e.to_string())))?;
        Ok(())
    }

    fn retrieve(&self, id: &str) -> Result<Option<Message>> {
        match self.db.get(id) {
            Ok(Some(data)) => {
                let message = Self::deserialize_message(&data)?;
                Ok(Some(message))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(MqttError::Persistence(PersistenceError::ReadFailed(
                e.to_string(),
            ))),
        }
    }

    fn delete(&self, id: &str) -> Result<()> {
        self.db
            .remove(id)
            .map_err(|e| MqttError::Persistence(PersistenceError::DeleteFailed(e.to_string())))?;
        Ok(())
    }

    fn list(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        for item in self.db.iter() {
            match item {
                Ok((key, _)) => {
                    if let Ok(id) = String::from_utf8(key.to_vec()) {
                        ids.push(id);
                    }
                }
                Err(e) => {
                    warn!("Error iterating persistence: {}", e);
                }
            }
        }
        Ok(ids)
    }

    fn clear(&self) -> Result<()> {
        self.db
            .clear()
            .map_err(|e| MqttError::Persistence(PersistenceError::WriteFailed(e.to_string())))?;
        Ok(())
    }

    fn count(&self) -> Result<usize> {
        Ok(self.db.len())
    }
}

/// Persistent publisher with automatic retry
pub struct PersistentPublisher {
    /// Base publisher
    publisher: Arc<Publisher>,
    /// Persistence backend
    persistence: Arc<dyn MessagePersistence>,
    /// Next message ID
    next_id: std::sync::atomic::AtomicU64,
}

impl PersistentPublisher {
    /// Create new persistent publisher
    pub fn new(publisher: Arc<Publisher>, persistence: Arc<dyn MessagePersistence>) -> Self {
        Self {
            publisher,
            persistence,
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Generate next message ID
    fn next_id(&self) -> String {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        format!("msg-{}", id)
    }

    /// Publish a message with persistence
    pub async fn publish(&self, message: Message) -> Result<()> {
        let id = self.next_id();

        // Store message first
        self.persistence.store(&id, &message)?;
        debug!("Stored message {} for publishing", id);

        // Attempt to publish
        match self.publisher.publish(message.clone()).await {
            Ok(()) => {
                // Success - remove from persistence
                self.persistence.delete(&id)?;
                debug!("Published and removed message {}", id);
                Ok(())
            }
            Err(e) => {
                // Failed - keep in persistence for retry
                error!("Failed to publish message {}: {}", id, e);
                Err(e)
            }
        }
    }

    /// Retry all failed messages
    pub async fn retry_failed(&self) -> Result<usize> {
        let ids = self.persistence.list()?;
        let total = ids.len();

        if total == 0 {
            return Ok(0);
        }

        info!("Retrying {} failed messages", total);

        let mut success = 0;
        for id in ids {
            if let Some(message) = self.persistence.retrieve(&id)? {
                match self.publisher.publish(message).await {
                    Ok(()) => {
                        self.persistence.delete(&id)?;
                        success += 1;
                        debug!("Retry succeeded for message {}", id);
                    }
                    Err(e) => {
                        warn!("Retry failed for message {}: {}", id, e);
                    }
                }
            }
        }

        info!("Retry complete: {}/{} messages published", success, total);

        Ok(success)
    }

    /// Get count of pending messages
    pub fn pending_count(&self) -> Result<usize> {
        self.persistence.count()
    }

    /// Clear all pending messages
    pub fn clear_pending(&self) -> Result<()> {
        self.persistence.clear()
    }

    /// Get persistence backend
    pub fn persistence(&self) -> &Arc<dyn MessagePersistence> {
        &self.persistence
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::types::QoS;

    #[test]
    fn test_in_memory_persistence() {
        let persistence = InMemoryPersistence::new();
        let message = Message::new("test/topic", b"hello".to_vec()).with_qos(QoS::AtLeastOnce);

        // Store
        assert!(persistence.store("msg1", &message).is_ok());

        // Retrieve
        let retrieved = persistence.retrieve("msg1").expect("Failed to retrieve");
        assert!(retrieved.is_some());
        let retrieved_msg = retrieved.expect("Message not found");
        assert_eq!(retrieved_msg.topic, "test/topic");
        assert_eq!(retrieved_msg.payload, b"hello");

        // Count
        assert_eq!(persistence.count().ok(), Some(1));

        // List
        let ids = persistence.list().expect("Failed to list");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], "msg1");

        // Delete
        assert!(persistence.delete("msg1").is_ok());
        assert_eq!(persistence.count().ok(), Some(0));
    }

    #[test]
    fn test_persistence_clear() {
        let persistence = InMemoryPersistence::new();
        let msg1 = Message::new("topic1", b"data1".to_vec());
        let msg2 = Message::new("topic2", b"data2".to_vec());

        persistence.store("msg1", &msg1).ok();
        persistence.store("msg2", &msg2).ok();

        assert_eq!(persistence.count().ok(), Some(2));

        persistence.clear().ok();
        assert_eq!(persistence.count().ok(), Some(0));
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_sled_persistence() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let persistence =
            SledPersistence::new(temp_dir.path()).expect("Failed to create persistence");

        let message = Message::new("test/topic", b"hello".to_vec()).with_qos(QoS::AtLeastOnce);

        // Store
        assert!(persistence.store("msg1", &message).is_ok());

        // Retrieve
        let retrieved = persistence.retrieve("msg1").expect("Failed to retrieve");
        assert!(retrieved.is_some());

        // Delete
        assert!(persistence.delete("msg1").is_ok());
        assert!(
            persistence
                .retrieve("msg1")
                .expect("Failed to retrieve")
                .is_none()
        );
    }
}
